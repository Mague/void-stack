//! Semantic search: embedding model, HNSW cache, and search API.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::hnswio::HnswIo;
use hnsw_rs::prelude::*;
use serde::Serialize;

use super::stats::{file_mtime, hnsw_dir, index_exists};
use crate::error::IndexError;
use crate::model::Project;

// ── Global caches ──────────────────────────────────────────

/// Cached embedding model — initialized once, reused across all calls.
static EMBEDDING_MODEL: OnceLock<Mutex<fastembed::TextEmbedding>> = OnceLock::new();

/// Cached HNSW indexes per project — loaded from disk once, invalidated on re-index.
static HNSW_CACHE: OnceLock<Mutex<HashMap<String, CachedHnsw>>> = OnceLock::new();

struct CachedHnsw {
    hnsw: Hnsw<'static, f32, DistCosine>,
    loaded_mtime: f64,
}

/// HNSW parameters.
pub(crate) const HNSW_MAX_CONN: usize = 16;
pub(crate) const HNSW_MAX_LAYERS: usize = 16;
pub(crate) const HNSW_EF_CONSTRUCTION: usize = 200;

/// Default HNSW search beam width — see the comment at the call site in
/// [`semantic_search`] for the recall/latency trade-off.
pub(crate) const DEFAULT_EF_SEARCH: usize = 64;

/// Upper bound for a user-configured `ef_search` — beyond this the search
/// degenerates to near-exhaustive scans with no recall benefit, so an
/// absurd `.void-config` value can't burn CPU unbounded.
pub(crate) const MAX_EF_SEARCH: usize = 1024;

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub file_path: String,
    pub chunk: String,
    pub score: f32,
    pub line_start: usize,
    pub line_end: usize,
    /// Community id from Leiden clustering (None if cluster_project hasn't run).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub community_id: Option<usize>,
}

// ── Public API ──────────────────────────────────────────────

/// Search mode: hybrid (BM25 + vector via Reciprocal Rank Fusion, the
/// default), vector-only, or lexical-only (BM25 over the FTS5 index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Hybrid,
    Vector,
    Lexical,
}

impl SearchMode {
    pub fn parse(s: &str) -> SearchMode {
        match s.to_ascii_lowercase().as_str() {
            "vector" => SearchMode::Vector,
            "lexical" | "bm25" | "text" => SearchMode::Lexical,
            _ => SearchMode::Hybrid,
        }
    }
}

/// RRF constant — standard k=60 dampens the head of each ranking.
const RRF_K: f32 = 60.0;
/// Lexical weight multiplier for exact-identifier queries.
const IDENTIFIER_LEXICAL_BOOST: f32 = 2.0;

/// Semantic search across the indexed codebase — hybrid by default.
pub fn semantic_search(
    project: &Project,
    query: &str,
    top_k: usize,
) -> Result<Vec<SearchResult>, IndexError> {
    search_with_mode(project, query, top_k, SearchMode::Hybrid)
}

/// Search with an explicit mode. Scores stay COSINE similarities in every
/// mode (downstream relevance floors depend on that semantics): fusion only
/// decides ordering; lexical-only hits get their cosine computed from the
/// stored chunk embedding.
pub fn search_with_mode(
    project: &Project,
    query: &str,
    top_k: usize,
    mode: SearchMode,
) -> Result<Vec<SearchResult>, IndexError> {
    if !index_exists(project) {
        return Err(IndexError::IndexNotFound(project.name.clone()));
    }
    let conn = super::db::open_meta_db(project)?;
    let fetch = (top_k * 3).max(10);

    let (vector_ranked, query_emb): (Vec<(i64, f32)>, Option<Vec<f32>>) =
        if mode != SearchMode::Lexical {
            let emb = embed_query(query)?;
            (vector_rank(project, &conn, &emb, fetch)?, Some(emb))
        } else {
            (Vec::new(), None)
        };
    let lexical_ranked: Vec<i64> = if mode != SearchMode::Vector {
        lexical_rank(&conn, query, fetch).unwrap_or_default()
    } else {
        Vec::new()
    };

    let lexical_weight = if is_identifier_query(query) {
        IDENTIFIER_LEXICAL_BOOST
    } else {
        1.0
    };
    let vector_ids: Vec<i64> = vector_ranked.iter().map(|(id, _)| *id).collect();
    let fused_ids = rrf_fuse(&vector_ids, &lexical_ranked, RRF_K, lexical_weight);

    let vector_scores: HashMap<i64, f32> = vector_ranked.into_iter().collect();
    let communities = super::cluster::load_communities(&conn).unwrap_or_default();

    let mut results: Vec<SearchResult> = Vec::with_capacity(top_k);
    for chunk_id in fused_ids.into_iter().take(top_k) {
        let Ok(chunk) = super::db::load_chunk_by_id(&conn, chunk_id) else {
            continue;
        };
        // Cosine score: from the HNSW result when present, otherwise from
        // the stored embedding (lexical-only hits).
        let score = match vector_scores.get(&chunk_id) {
            Some(s) => *s,
            None => query_emb
                .as_deref()
                .and_then(|qe| chunk_embedding_cosine(&conn, chunk_id, qe))
                .unwrap_or(0.0),
        };
        results.push(SearchResult {
            file_path: chunk.file_path,
            chunk: chunk.text,
            score,
            line_start: chunk.line_start,
            line_end: chunk.line_end,
            community_id: communities.get(&chunk_id).copied(),
        });
    }

    // Record *real* savings: tokens in the returned chunks vs the full
    // size of every unique file we drew them from.
    record_real_search_savings(project, "semantic_search", &results);

    Ok(results)
}

/// Embed the query text (cached model).
fn embed_query(query: &str) -> Result<Vec<f32>, IndexError> {
    let mut embs = embed_texts(&[query.to_string()])?;
    if embs.is_empty() {
        return Err(IndexError::EmbeddingFailed(
            "failed to generate query embedding".to_string(),
        ));
    }
    Ok(embs.remove(0))
}

/// Vector side: HNSW neighbours as ranked (chunk_id, cosine) pairs.
fn vector_rank(
    project: &Project,
    conn: &rusqlite::Connection,
    query_emb: &[f32],
    fetch: usize,
) -> Result<Vec<(i64, f32)>, IndexError> {
    let chunk_order = super::db::load_chunk_order(conn)?;

    let cache_key = ensure_hnsw_cached(project)?;
    let hnsw_cache = HNSW_CACHE
        .get()
        .ok_or_else(|| IndexError::HnswIo("HNSW cache not initialized".to_string()))?;
    let hnsw_map = hnsw_cache
        .lock()
        .map_err(|e| IndexError::HnswIo(format!("HNSW cache lock poisoned: {}", e)))?;
    let cached = hnsw_map
        .get(&cache_key)
        .ok_or_else(|| IndexError::HnswIo("HNSW index not in cache".to_string()))?;

    // ef_search controls the HNSW search beam width: the recall/latency
    // trade-off knob. 64 gives near-exhaustive recall for code-search-sized
    // indexes; override per project with `[index] ef_search = N`.
    let configured =
        crate::project_config::ProjectConfig::load(std::path::Path::new(&project.path))
            .index
            .ef_search
            .unwrap_or(DEFAULT_EF_SEARCH)
            .min(MAX_EF_SEARCH);
    let ef_search = fetch.max(configured);
    let neighbours = cached.hnsw.search(query_emb, fetch, ef_search);

    let mut out = Vec::with_capacity(neighbours.len());
    for neighbour in &neighbours {
        if let Some(chunk_id) = chunk_order.get(neighbour.d_id) {
            out.push((*chunk_id, 1.0 - neighbour.distance));
        }
    }
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}

/// Lexical side: BM25-ranked chunk ids from the FTS5 index.
fn lexical_rank(
    conn: &rusqlite::Connection,
    query: &str,
    fetch: usize,
) -> Result<Vec<i64>, IndexError> {
    let Some(match_q) = build_fts_query(query) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn
        .prepare(
            "SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1 \
             ORDER BY bm25(chunks_fts) LIMIT ?2",
        )
        .map_err(IndexError::from)?;
    let rows = stmt
        .query_map(rusqlite::params![match_q, fetch as i64], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(IndexError::from)?;
    Ok(rows.flatten().collect())
}

/// Build a safe FTS5 MATCH expression: identifier-ish queries become a
/// quoted phrase; everything else an OR of sanitized tokens (FTS5's
/// implicit AND returns nothing for long conceptual queries).
fn build_fts_query(query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|t| t.len() > 1)
        .map(|t| t.to_string())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    if is_identifier_query(query) {
        return Some(format!("\"{}\"", tokens[0]));
    }
    Some(
        tokens
            .iter()
            .map(|t| format!("\"{}\"", t))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Single-token snake_case/CamelCase or explicitly quoted queries are
/// identifier lookups — lexical evidence outweighs embeddings for those.
fn is_identifier_query(query: &str) -> bool {
    let t = query.trim();
    if t.starts_with('"') && t.ends_with('"') && t.len() > 2 {
        return true;
    }
    if t.split_whitespace().count() != 1 {
        return false;
    }
    let has_underscore = t.contains('_');
    let mixed_case =
        t.chars().any(|c| c.is_ascii_uppercase()) && t.chars().any(|c| c.is_ascii_lowercase());
    has_underscore || mixed_case
}

/// Reciprocal Rank Fusion: score(id) = Σ weight / (k + rank). Stable for
/// ids present in only one list.
fn rrf_fuse(vector_ids: &[i64], lexical_ids: &[i64], k: f32, lexical_weight: f32) -> Vec<i64> {
    let mut scores: HashMap<i64, f32> = HashMap::new();
    for (rank, id) in vector_ids.iter().enumerate() {
        *scores.entry(*id).or_default() += 1.0 / (k + rank as f32 + 1.0);
    }
    for (rank, id) in lexical_ids.iter().enumerate() {
        *scores.entry(*id).or_default() += lexical_weight / (k + rank as f32 + 1.0);
    }
    let mut out: Vec<(i64, f32)> = scores.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    out.into_iter().map(|(id, _)| id).collect()
}

/// Cosine between the query embedding and a chunk's STORED embedding —
/// used to give lexical-only hits a real similarity score.
fn chunk_embedding_cosine(
    conn: &rusqlite::Connection,
    chunk_id: i64,
    query_emb: &[f32],
) -> Option<f32> {
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT embedding FROM chunks WHERE id = ?1 AND embedding IS NOT NULL",
            [chunk_id],
            |r| r.get(0),
        )
        .ok()?;
    let emb = super::db::bytes_to_f32_vec(&blob);
    if emb.len() != query_emb.len() || emb.is_empty() {
        return None;
    }
    let dot: f32 = emb.iter().zip(query_emb).map(|(a, b)| a * b).sum();
    let na: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = query_emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return None;
    }
    Some(dot / (na * nb))
}

/// Pure savings calculator: returned tokens, full-file tokens, and the
/// resulting savings pct. Extracted from `record_real_search_savings` so
/// it can be tested without touching the on-disk stats db.
///
/// Approximates tokens as bytes/4. Unique source files are counted once
/// — multiple chunks from the same file don't double-count on the "full"
/// side. Returns `(tokens_full, tokens_returned, savings_pct)`.
pub(crate) fn compute_search_savings(
    project_root: &std::path::Path,
    chunks_and_files: &[(&str, &str)],
) -> (usize, usize, f32) {
    let tokens_returned: usize = chunks_and_files
        .iter()
        .map(|(chunk, _)| chunk.len() / 4)
        .sum();

    let mut unique_files: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (_, file) in chunks_and_files {
        unique_files.insert(*file);
    }
    let tokens_full: usize = unique_files
        .iter()
        .filter_map(|p| std::fs::metadata(project_root.join(p)).ok())
        .map(|m| m.len() as usize / 4)
        .sum();

    let savings_pct = if tokens_full > tokens_returned {
        ((1.0 - tokens_returned as f64 / tokens_full as f64) * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    };

    (tokens_full, tokens_returned, savings_pct)
}

/// Shared savings recorder used by both `semantic_search` and
/// `graph_rag_search`. Computes via [`compute_search_savings`] and writes
/// one row to the token_savings table.
///
/// Runs on a detached background thread: telemetry involves fs metadata
/// reads plus a stats-DB write and must never add latency to search nor
/// affect its results — failures are silently dropped.
pub(crate) fn record_real_search_savings(
    project: &Project,
    operation: &str,
    results: &[SearchResult],
) {
    let project_path = project.path.clone();
    let project_name = project.name.clone();
    let operation = operation.to_string();
    let pairs: Vec<(String, String)> = results
        .iter()
        .map(|r| (r.chunk.clone(), r.file_path.clone()))
        .collect();

    std::thread::spawn(move || {
        let borrowed: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(c, f)| (c.as_str(), f.as_str()))
            .collect();
        let (tokens_full, tokens_returned, savings_pct) =
            compute_search_savings(std::path::Path::new(&project_path), &borrowed);

        crate::stats::record_saving(crate::stats::TokenSavingsRecord {
            timestamp: Utc::now(),
            project: project_name,
            operation,
            lines_original: tokens_full,
            lines_filtered: tokens_returned,
            savings_pct,
        });
    });
}

// ── Embedding model ─────────────────────────────────────────

/// Get or initialize the cached embedding model.
fn get_embedding_model() -> Result<&'static Mutex<fastembed::TextEmbedding>, IndexError> {
    if let Some(m) = EMBEDDING_MODEL.get() {
        return Ok(m);
    }

    // First-time init: cap ONNX/OpenMP intra-op thread pools so fastembed
    // doesn't spawn a thread per logical CPU and blow past the indexer's
    // dedicated rayon pool. We only run this branch before EMBEDDING_MODEL
    // is set, so the env vars are written exactly once per process and
    // before ONNX reads them. set_var is unsafe under edition 2024 because
    // it races with concurrent getenv readers; the OnceLock guard above
    // means at most one indexer thread reaches this point.
    let threads = super::indexer::indexing_rayon_threads().to_string();
    // SAFETY: written before any ONNX init, and the OnceLock guard
    // serializes initialization across threads.
    unsafe {
        std::env::set_var("OMP_NUM_THREADS", &threads);
        std::env::set_var("ORT_NUM_THREADS", &threads);
    }

    let cache_dir = super::stats::model_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);

    let options = fastembed::InitOptions::new(fastembed::EmbeddingModel::BGESmallENV15)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(true);

    let model = fastembed::TextEmbedding::try_new(options)
        .map_err(|e| IndexError::EmbeddingFailed(format!("model init error: {}", e)))?;

    // Race-safe: if another thread initialized first, use theirs
    let _ = EMBEDDING_MODEL.set(Mutex::new(model));
    EMBEDDING_MODEL
        .get()
        .ok_or_else(|| IndexError::EmbeddingFailed("embedding model not initialized".to_string()))
}

/// Embed texts using the cached model.
pub(crate) fn embed_texts(texts: &[String]) -> Result<Vec<Vec<f32>>, IndexError> {
    let model_lock = get_embedding_model()?;
    let model = model_lock
        .lock()
        .map_err(|e| IndexError::EmbeddingFailed(format!("model lock poisoned: {}", e)))?;
    model
        .embed(texts.to_vec(), None)
        .map_err(|e| IndexError::EmbeddingFailed(format!("embedding error: {}", e)))
}

/// Ensure the HNSW index is loaded into cache for a project. Returns the cache key.
fn ensure_hnsw_cached(project: &Project) -> Result<String, IndexError> {
    let cache = HNSW_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let hnsw_path = hnsw_dir(project);
    let key = hnsw_path.to_string_lossy().to_string();
    let data_file = hnsw_path.join("index_data.hnsw");
    let current_mtime = file_mtime(&data_file);

    let mut map = cache
        .lock()
        .map_err(|e| IndexError::HnswIo(format!("HNSW cache lock poisoned: {}", e)))?;

    let needs_load = match map.get(&key) {
        Some(cached) => cached.loaded_mtime < current_mtime,
        None => true,
    };

    if needs_load {
        // Leak HnswIo so the loaded Hnsw gets 'static lifetime.
        // This is intentional — cached indexes live for the process lifetime.
        let io = Box::leak(Box::new(HnswIo::new(&hnsw_path, "index")));
        let hnsw: Hnsw<'static, f32, DistCosine> = io
            .load_hnsw()
            .map_err(|e| IndexError::HnswIo(format!("failed to load HNSW index: {}", e)))?;

        // Validate the index has points — an empty HNSW silently returns 0 results
        let nb_points = hnsw.get_nb_point();
        if nb_points == 0 {
            return Err(IndexError::HnswIo(format!(
                "HNSW index at '{}' is empty (0 points). \
                 The index may be corrupted or still building. \
                 Run index_project_codebase with force=true to rebuild.",
                hnsw_path.display()
            )));
        }

        map.insert(
            key.clone(),
            CachedHnsw {
                hnsw,
                loaded_mtime: current_mtime,
            },
        );
    }

    Ok(key)
}

/// Invalidate the HNSW cache entry for a project (called after re-indexing).
pub(crate) fn invalidate_hnsw_cache(project: &Project) {
    if let Some(cache) = HNSW_CACHE.get()
        && let Ok(mut map) = cache.lock()
    {
        let key = hnsw_dir(project).to_string_lossy().to_string();
        map.remove(&key);
    }
}

#[cfg(test)]
mod hybrid_tests {
    use super::*;

    #[test]
    fn test_is_identifier_query() {
        assert!(is_identifier_query("stop_unix_process_group"));
        assert!(is_identifier_query("ProcessManager"));
        assert!(is_identifier_query("\"anything quoted\""));
        assert!(!is_identifier_query("how do we authenticate"));
        assert!(!is_identifier_query("lowercase"));
    }

    #[test]
    fn test_build_fts_query_forms() {
        assert_eq!(
            build_fts_query("stop_unix_process_group").as_deref(),
            Some("\"stop_unix_process_group\"")
        );
        assert_eq!(
            build_fts_query("how do we authenticate users").as_deref(),
            Some("\"how\" OR \"do\" OR \"we\" OR \"authenticate\" OR \"users\"")
        );
        assert_eq!(build_fts_query("?!"), None);
    }

    #[test]
    fn test_rrf_fuse_ordering_and_weight() {
        // id 1 leads vector, id 3 leads lexical; id 2 is mid in both.
        let vector = vec![1, 2, 3];
        let lexical = vec![3, 2];

        // Equal weights: id appearing high in both (or in both lists) wins.
        let fused = rrf_fuse(&vector, &lexical, 60.0, 1.0);
        assert_eq!(fused.len(), 3);
        // id3: 1/(60+3) + 1/(60+1) ≈ 0.0323 ; id2: 1/62 + 1/62 ≈ 0.0322 ; id1: 1/61 ≈ 0.0164
        assert_eq!(fused[0], 3);
        assert_eq!(fused[2], 1);

        // Lexical boost pushes the lexical leader further ahead.
        let fused = rrf_fuse(&vector, &lexical, 60.0, 2.0);
        assert_eq!(fused[0], 3, "boosted lexical leader must rank first");
    }

    /// An exact identifier must rank #1 via the lexical index even when
    /// the vector side misses it entirely (no embeddings in this test —
    /// pure FTS over a real meta.db).
    #[test]
    fn test_exact_identifier_ranks_first_lexically() {
        let dir = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: format!("hybrid-fixture-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let conn = super::super::db::open_meta_db(&project).unwrap();
        for (file, text) in [
            (
                "a.rs",
                "fn unrelated_helper() { compute_totals(); } // computes report totals",
            ),
            (
                "b.rs",
                "fn stop_unix_process_group(pid: u32) { libc::kill(-(pid as i32), SIGTERM); }",
            ),
            ("c.rs", "fn another_thing() {}"),
        ] {
            conn.execute(
                "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
                 VALUES (?1, 1, 10, ?2, 0)",
                rusqlite::params![file, text],
            )
            .unwrap();
            super::super::db::fts_replace_file(&conn, file).unwrap();
        }

        let ranked = lexical_rank(&conn, "stop_unix_process_group", 10).unwrap();
        assert_eq!(ranked.len(), 1, "only the defining chunk matches");
        let chunk = super::super::db::load_chunk_by_id(&conn, ranked[0]).unwrap();
        assert_eq!(chunk.file_path, "b.rs");

        // Conceptual multi-word query still returns results (OR semantics).
        let ranked = lexical_rank(&conn, "compute the totals helper", 10).unwrap();
        assert!(!ranked.is_empty());
    }
}

#[cfg(test)]
mod savings_tests {
    use super::*;

    #[test]
    fn test_savings_pct_real_file() {
        // 1000-byte source file, 50-byte returned chunk → ~95% savings.
        let dir = tempfile::tempdir().unwrap();
        let file_rel = "src/example.rs";
        let full_path = dir.path().join(file_rel);
        std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        std::fs::write(&full_path, "x".repeat(1000)).unwrap();

        let chunk = "y".repeat(50);
        let pairs = vec![(chunk.as_str(), file_rel)];
        let (tokens_full, tokens_returned, pct) = compute_search_savings(dir.path(), &pairs);

        assert_eq!(tokens_full, 250, "1000 bytes / 4 = 250 tokens");
        assert_eq!(tokens_returned, 12, "50 bytes / 4 = 12 tokens");
        assert!((pct - 95.0).abs() < 1.0, "expected ~95% savings, got {pct}");
    }

    #[test]
    fn test_savings_unique_files_not_double_counted() {
        // Two chunks from the same 800-byte file → tokens_full counts the
        // file once, not twice.
        let dir = tempfile::tempdir().unwrap();
        let file_rel = "src/a.rs";
        let full_path = dir.path().join(file_rel);
        std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        std::fs::write(&full_path, "x".repeat(800)).unwrap();

        let c1 = "a".repeat(40);
        let c2 = "b".repeat(40);
        let pairs = vec![(c1.as_str(), file_rel), (c2.as_str(), file_rel)];
        let (tokens_full, _, _) = compute_search_savings(dir.path(), &pairs);
        assert_eq!(tokens_full, 200, "800/4 once, not 400");
    }

    #[test]
    fn test_savings_pct_zero_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let chunk = "x".repeat(100);
        let pairs = vec![(chunk.as_str(), "ghost.rs")];
        let (tokens_full, _, pct) = compute_search_savings(dir.path(), &pairs);
        assert_eq!(tokens_full, 0);
        assert_eq!(pct, 0.0);
    }
}
