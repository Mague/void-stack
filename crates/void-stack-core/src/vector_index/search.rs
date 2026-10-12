//! Semantic search: embedding model, HNSW cache, and search API.

use std::collections::HashMap;
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::hnswio::HnswIo;
use hnsw_rs::prelude::*;
use serde::Serialize;

use super::stats::{file_mtime, hnsw_dir, hnsw_graph_file, index_exists};
use crate::error::IndexError;
use crate::model::Project;

// ── Global caches ──────────────────────────────────────────
//
// Both caches are bounded: a long-lived process (desktop app, MCP server)
// used to keep every HNSW index it ever touched plus the ONNX model and its
// arena resident forever — a cross-project search across ~30 indexed
// projects pinned all of them in RAM (multi-GB). Now:
//   * at most `hnsw_cache_capacity()` HNSW indexes stay resident (LRU);
//   * entries idle for `HNSW_IDLE_TTL` are dropped by the reaper;
//   * the embedding model is dropped after `MODEL_IDLE_TTL` without use
//     and transparently re-created on the next embed call.

/// Cached embedding model — created on demand, dropped when idle.
static EMBEDDING_MODEL: Mutex<Option<ModelSlot>> = Mutex::new(None);

struct ModelSlot {
    model: fastembed::TextEmbedding,
    last_used: Instant,
}

/// Cached HNSW indexes per project (LRU-bounded, idle-evicted).
static HNSW_CACHE: OnceLock<Mutex<HashMap<String, CachedHnsw>>> = OnceLock::new();

/// Default number of HNSW indexes kept resident at once. Searches mostly
/// target the selected project, so a small LRU keeps the hot path cached
/// without pinning every indexed project. Override with
/// `VOID_STACK_HNSW_CACHE=<n>` (min 1).
const DEFAULT_HNSW_CACHE_CAPACITY: usize = 2;

/// A cached HNSW index is dropped after this long without a search.
const HNSW_IDLE_TTL: Duration = Duration::from_secs(10 * 60);

/// The embedding model (and the ONNX arena it grew while indexing) is
/// dropped after this long without an embed call.
const MODEL_IDLE_TTL: Duration = Duration::from_secs(5 * 60);

/// How often the background reaper checks for idle entries.
const REAPER_INTERVAL: Duration = Duration::from_secs(60);

/// A loaded HNSW graph together with the reader it borrows from.
///
/// `hnsw` borrows from the reader behind `io` (and, when memory-mapped,
/// from the mapping the reader owns), so `Drop` releases the graph first
/// and only then frees the reader.
struct CachedHnsw {
    hnsw: std::mem::ManuallyDrop<Hnsw<'static, f32, DistCosine>>,
    io: std::ptr::NonNull<HnswIo>,
    /// hnsw_id → chunk_id, loaded alongside the graph so a search doesn't
    /// re-read the whole `chunk_order` table every time.
    chunk_order: Vec<i64>,
    loaded_mtime: f64,
    last_used: Instant,
}

// SAFETY: `io` is uniquely owned by this entry (it's a leaked `Box` that
// only `Drop` turns back into one), so moving the entry between threads
// moves exclusive ownership of the reader with it; `HnswIo` itself is Send.
unsafe impl Send for CachedHnsw {}

const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<HnswIo>();
};

impl Drop for CachedHnsw {
    fn drop(&mut self) {
        // SAFETY: the graph is dropped exactly once, here, before the reader
        // it borrows from; `io` came from `Box::into_raw` in `load` and is
        // not used after this point.
        unsafe {
            std::mem::ManuallyDrop::drop(&mut self.hnsw);
            drop(Box::from_raw(self.io.as_ptr()));
        }
    }
}

impl CachedHnsw {
    /// Load the HNSW graph for `project` from disk.
    ///
    /// `mmap` maps the vectors from the data file instead of copying them
    /// onto the heap — used for transient loads (cross-project fan-out), so
    /// a sweep over dozens of projects doesn't allocate every vector. Not
    /// used for cached entries: on Windows a live mapping would block the
    /// indexer from replacing the index directory.
    fn load(
        project: &Project,
        conn: &rusqlite::Connection,
        mmap: bool,
    ) -> Result<Self, IndexError> {
        let hnsw_path = hnsw_dir(project);
        let loaded_mtime = file_mtime(&hnsw_graph_file(project));
        // Load the id mapping before the graph: the indexer writes
        // `chunk_order` before dumping the graph, so a graph we read here is
        // never older than this mapping (a newer graph bumps the mtime and
        // forces a reload on the next search).
        let chunk_order = super::db::load_chunk_order(conn)?;

        let options = hnsw_rs::hnswio::ReloadOptions::default().set_mmap(mmap);
        let io = std::ptr::NonNull::from(Box::leak(Box::new(HnswIo::new_with_options(
            &hnsw_path, "index", options,
        ))));
        // SAFETY: the graph borrows from the reader (and its mapping). The
        // reader lives on the heap at a stable address, is owned only by
        // the returned entry (or freed right below on error), and `Drop`
        // releases the graph before the reader — the borrow never outlives
        // it. `load_hnsw` needs `&mut` only while loading; nothing touches
        // the reader again until `Drop`.
        let loaded = unsafe { (*io.as_ptr()).load_hnsw::<f32, DistCosine>() };
        let hnsw: Hnsw<'static, f32, DistCosine> = match loaded {
            Ok(h) => h,
            Err(e) => {
                // SAFETY: no graph borrows from the reader on this path.
                unsafe { drop(Box::from_raw(io.as_ptr())) };
                return Err(IndexError::HnswIo(format!(
                    "failed to load HNSW index: {}",
                    e
                )));
            }
        };
        let entry = CachedHnsw {
            hnsw: std::mem::ManuallyDrop::new(hnsw),
            io,
            chunk_order,
            loaded_mtime,
            last_used: Instant::now(),
        };

        // Validate the index has points — an empty HNSW silently returns 0 results
        if entry.hnsw.get_nb_point() == 0 {
            return Err(IndexError::HnswIo(format!(
                "HNSW index at '{}' is empty (0 points). \
                 The index may be corrupted or still building. \
                 Run index_project_codebase with force=true to rebuild.",
                hnsw_path.display()
            )));
        }

        Ok(entry)
    }
}

/// Whether a search should keep the loaded index resident afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachePolicy {
    /// Keep it in the LRU cache (the project the user is working on).
    Keep,
    /// Use a cached copy if present, otherwise load, search and drop.
    /// Used for fan-out searches over many projects (cross-project
    /// GraphRAG) so one query doesn't pin every index in memory.
    Transient,
}

fn hnsw_cache() -> &'static Mutex<HashMap<String, CachedHnsw>> {
    HNSW_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Configured LRU capacity (`VOID_STACK_HNSW_CACHE`, default 2, min 1).
pub(crate) fn hnsw_cache_capacity() -> usize {
    parse_capacity(std::env::var("VOID_STACK_HNSW_CACHE").ok().as_deref())
}

fn parse_capacity(raw: Option<&str>) -> usize {
    raw.and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(DEFAULT_HNSW_CACHE_CAPACITY)
}

/// Drop least-recently-used entries until at most `capacity` remain.
fn evict_lru<V>(map: &mut HashMap<String, V>, capacity: usize, last_used: impl Fn(&V) -> Instant) {
    while map.len() > capacity {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, v)| last_used(v))
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
}

/// Drop entries idle for longer than `ttl`. Returns how many were dropped.
fn evict_idle<V>(
    map: &mut HashMap<String, V>,
    ttl: Duration,
    now: Instant,
    last_used: impl Fn(&V) -> Instant,
) -> usize {
    let before = map.len();
    map.retain(|_, v| now.saturating_duration_since(last_used(v)) < ttl);
    before - map.len()
}

/// Start the idle reaper once per process. It wakes every
/// `REAPER_INTERVAL` and drops idle HNSW indexes and the idle model.
fn ensure_reaper() {
    static REAPER: Once = Once::new();
    REAPER.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("void-index-reaper".to_string())
            .spawn(|| {
                loop {
                    std::thread::sleep(REAPER_INTERVAL);
                    reap_idle(Instant::now());
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("could not start index cache reaper: {}", e);
        }
    });
}

/// One reaper pass (split out so tests can drive it deterministically).
fn reap_idle(now: Instant) {
    if let Ok(mut map) = hnsw_cache().lock() {
        let dropped = evict_idle(&mut map, HNSW_IDLE_TTL, now, |c| c.last_used);
        if dropped > 0 {
            tracing::debug!("released {} idle HNSW index(es)", dropped);
        }
    }
    if let Ok(mut slot) = EMBEDDING_MODEL.lock()
        && slot
            .as_ref()
            .is_some_and(|m| now.saturating_duration_since(m.last_used) >= MODEL_IDLE_TTL)
    {
        *slot = None;
        tracing::debug!("released idle embedding model");
    }
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
    search_with_policy(project, query, None, top_k, mode, CachePolicy::Keep)
}

/// Hybrid search that does not keep `project`'s HNSW index resident —
/// for fan-out over many projects (see [`CachePolicy::Transient`]). Takes
/// the query embedding precomputed so a fan-out embeds the query once,
/// not once per project.
pub(crate) fn semantic_search_transient(
    project: &Project,
    query: &str,
    query_emb: &[f32],
    top_k: usize,
) -> Result<Vec<SearchResult>, IndexError> {
    search_with_policy(
        project,
        query,
        Some(query_emb),
        top_k,
        SearchMode::Hybrid,
        CachePolicy::Transient,
    )
}

fn search_with_policy(
    project: &Project,
    query: &str,
    precomputed_emb: Option<&[f32]>,
    top_k: usize,
    mode: SearchMode,
    policy: CachePolicy,
) -> Result<Vec<SearchResult>, IndexError> {
    if !index_exists(project) {
        return Err(IndexError::IndexNotFound(project.name.clone()));
    }
    let conn = super::db::open_meta_db(project)?;
    let fetch = (top_k * 3).max(10);

    let (vector_ranked, query_emb): (Vec<(i64, f32)>, Option<Vec<f32>>) =
        if mode != SearchMode::Lexical {
            let emb = match precomputed_emb {
                Some(e) => e.to_vec(),
                None => embed_query(query)?,
            };
            (vector_rank(project, &conn, &emb, fetch, policy)?, Some(emb))
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
    let top_ids: Vec<i64> = fused_ids.into_iter().take(top_k).collect();
    // Only the communities of the chunks we return — not the whole table.
    let communities = super::cluster::load_communities_for(&conn, &top_ids).unwrap_or_default();

    let mut results: Vec<SearchResult> = Vec::with_capacity(top_k);
    for chunk_id in top_ids {
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
pub(crate) fn embed_query(query: &str) -> Result<Vec<f32>, IndexError> {
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
    policy: CachePolicy,
) -> Result<Vec<(i64, f32)>, IndexError> {
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

    with_hnsw(project, conn, policy, |cached| {
        let neighbours = cached.hnsw.search(query_emb, fetch, ef_search);
        let mut out = Vec::with_capacity(neighbours.len());
        for neighbour in &neighbours {
            if let Some(chunk_id) = cached.chunk_order.get(neighbour.d_id) {
                out.push((*chunk_id, 1.0 - neighbour.distance));
            }
        }
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    })
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
pub fn is_identifier_query(query: &str) -> bool {
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

/// Cap ONNX/OpenMP intra-op thread pools so fastembed doesn't spawn a
/// thread per logical CPU and blow past the indexer's dedicated rayon pool.
/// Runs once per process, before the first model is created (ONNX reads
/// these at init). `set_var` is unsafe under edition 2024 because it races
/// with concurrent getenv readers; the `Once` serializes it.
fn configure_onnx_threads() {
    static ONNX_ENV: Once = Once::new();
    ONNX_ENV.call_once(|| {
        let threads = super::indexer::indexing_rayon_threads().to_string();
        // SAFETY: written exactly once, before any ONNX session exists.
        unsafe {
            std::env::set_var("OMP_NUM_THREADS", &threads);
            std::env::set_var("ORT_NUM_THREADS", &threads);
        }
    });
}

/// Create the embedding model (downloads it on first use).
fn create_embedding_model() -> Result<fastembed::TextEmbedding, IndexError> {
    configure_onnx_threads();

    let cache_dir = super::stats::model_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);

    // ONNX Runtime's CPU memory arena keeps its high-water mark for the life
    // of the session. Measured re-indexing a 3k-chunk project (batch 8,
    // length-sorted): arena on 108 s / 660 MB peak, off 205 s / 700 MB —
    // without the arena every run re-allocates and indexing takes ~2×.
    // The arena stays on; the idle reaper drops the whole session (and the
    // arena with it) after MODEL_IDLE_TTL. `VOID_STACK_ORT_ARENA=0` turns
    // it off for memory-starved machines.
    let mut cpu = ort::execution_providers::CPUExecutionProvider::default();
    if !std::env::var("VOID_STACK_ORT_ARENA").is_ok_and(|v| v == "0") {
        cpu = cpu.with_arena_allocator();
    }
    let options = fastembed::InitOptions::new(fastembed::EmbeddingModel::BGESmallENV15)
        .with_cache_dir(cache_dir.clone())
        .with_show_download_progress(true)
        .with_execution_providers(vec![cpu.build()]);

    fastembed::TextEmbedding::try_new(options).map_err(|e| {
        IndexError::EmbeddingFailed(format!(
            "model init error: {} (cache_dir={})",
            e,
            cache_dir.display()
        ))
    })
}

/// Embed texts using the cached model, creating it if it was never loaded
/// or was released for being idle.
pub(crate) fn embed_texts(texts: &[String]) -> Result<Vec<Vec<f32>>, IndexError> {
    let mut slot = EMBEDDING_MODEL
        .lock()
        .map_err(|e| IndexError::EmbeddingFailed(format!("model lock poisoned: {}", e)))?;
    if slot.is_none() {
        *slot = Some(ModelSlot {
            model: create_embedding_model()?,
            last_used: Instant::now(),
        });
        ensure_reaper();
    }
    let entry = slot.as_mut().ok_or_else(|| {
        IndexError::EmbeddingFailed("embedding model not initialized".to_string())
    })?;
    entry.last_used = Instant::now();
    entry
        .model
        .embed(texts.to_vec(), None)
        .map_err(|e| IndexError::EmbeddingFailed(format!("embedding error: {}", e)))
}

/// Run `f` against `project`'s HNSW index, loading it if it isn't cached
/// or the on-disk index is newer than the cached one.
///
/// With [`CachePolicy::Keep`] the index stays in the LRU cache afterwards
/// (evicting the least-recently-used entry beyond capacity); with
/// [`CachePolicy::Transient`] a freshly loaded index is dropped as soon as
/// `f` returns.
fn with_hnsw<R>(
    project: &Project,
    conn: &rusqlite::Connection,
    policy: CachePolicy,
    f: impl FnOnce(&CachedHnsw) -> R,
) -> Result<R, IndexError> {
    let key = hnsw_dir(project).to_string_lossy().to_string();
    let current_mtime = file_mtime(&hnsw_graph_file(project));

    let mut map = hnsw_cache()
        .lock()
        .map_err(|e| IndexError::HnswIo(format!("HNSW cache lock poisoned: {}", e)))?;

    if let Some(cached) = map.get_mut(&key)
        && cached.loaded_mtime >= current_mtime
    {
        cached.last_used = Instant::now();
        return Ok(f(cached));
    }
    // Stale or missing: drop any old copy before loading so two versions
    // of the same index never sit in memory at once.
    map.remove(&key);

    match policy {
        CachePolicy::Transient => {
            // Don't hold the global lock while reading from disk.
            drop(map);
            let loaded = CachedHnsw::load(project, conn, true)?;
            Ok(f(&loaded))
        }
        CachePolicy::Keep => {
            let loaded = CachedHnsw::load(project, conn, false)?;
            let out = f(&loaded);
            map.insert(key, loaded);
            evict_lru(&mut map, hnsw_cache_capacity(), |c| c.last_used);
            drop(map);
            ensure_reaper();
            Ok(out)
        }
    }
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
        crate::isolate_test_data_dir();
        assert!(is_identifier_query("stop_unix_process_group"));
        assert!(is_identifier_query("ProcessManager"));
        assert!(is_identifier_query("\"anything quoted\""));
        assert!(!is_identifier_query("how do we authenticate"));
        assert!(!is_identifier_query("lowercase"));
    }

    #[test]
    fn test_build_fts_query_forms() {
        crate::isolate_test_data_dir();
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
    fn test_search_mode_parse() {
        crate::isolate_test_data_dir();
        assert_eq!(SearchMode::parse("vector"), SearchMode::Vector);
        assert_eq!(SearchMode::parse("VECTOR"), SearchMode::Vector);
        assert_eq!(SearchMode::parse("lexical"), SearchMode::Lexical);
        assert_eq!(SearchMode::parse("bm25"), SearchMode::Lexical);
        assert_eq!(SearchMode::parse("text"), SearchMode::Lexical);
        assert_eq!(SearchMode::parse("hybrid"), SearchMode::Hybrid);
        // Unknown or empty strings fall back to the hybrid default.
        assert_eq!(SearchMode::parse(""), SearchMode::Hybrid);
        assert_eq!(SearchMode::parse("garbage"), SearchMode::Hybrid);
    }

    #[test]
    fn test_is_identifier_query_edge_cases() {
        crate::isolate_test_data_dir();
        // An empty quoted pair is too short to count as a quoted lookup.
        assert!(!is_identifier_query("\"\""));
        // Quoted multi-word queries are explicit phrase lookups (trimmed).
        assert!(is_identifier_query("  \"exact phrase\"  "));
        // ALL-CAPS single token: no lowercase, so not mixed-case.
        assert!(!is_identifier_query("HTTP"));
        // Single mixed-case word without an underscore is an identifier.
        assert!(is_identifier_query("camelCase"));
        // Multi-word stays conceptual even when a token has underscores.
        assert!(!is_identifier_query("stop the_process now"));
        // Whitespace-only input is not an identifier.
        assert!(!is_identifier_query("   "));
    }

    #[test]
    fn test_build_fts_query_edge_cases() {
        crate::isolate_test_data_dir();
        // Single-char tokens are dropped; the rest are OR'ed.
        assert_eq!(
            build_fts_query("a fn x of").as_deref(),
            Some("\"fn\" OR \"of\"")
        );
        // Nothing but 1-char tokens leaves no query at all.
        assert_eq!(build_fts_query("a b c"), None);
        // A quoted phrase is an identifier lookup: first token, quoted.
        assert_eq!(
            build_fts_query("\"exact phrase\"").as_deref(),
            Some("\"exact\"")
        );
        // CamelCase single token becomes a quoted phrase.
        assert_eq!(
            build_fts_query("ProcessManager").as_deref(),
            Some("\"ProcessManager\"")
        );
        // Punctuation acts as a separator so FTS5 syntax can't leak in.
        assert_eq!(
            build_fts_query("foo() OR bar-baz").as_deref(),
            Some("\"foo\" OR \"OR\" OR \"bar\" OR \"baz\"")
        );
    }

    #[test]
    fn test_rrf_fuse_empty_and_single_list() {
        crate::isolate_test_data_dir();
        assert!(rrf_fuse(&[], &[], 60.0, 1.0).is_empty());
        // Only the vector list: input order is preserved.
        assert_eq!(rrf_fuse(&[7, 3, 9], &[], 60.0, 1.0), vec![7, 3, 9]);
        // Only the lexical list: order preserved even with a boost.
        assert_eq!(rrf_fuse(&[], &[4, 2], 60.0, 2.0), vec![4, 2]);
    }

    #[test]
    fn test_rrf_fuse_tie_breaks_by_ascending_id() {
        crate::isolate_test_data_dir();
        // Symmetric ranks give ids 1 and 2 identical scores; the tie must
        // break deterministically by ascending id.
        let fused = rrf_fuse(&[1, 2], &[2, 1], 60.0, 1.0);
        assert_eq!(fused, vec![1, 2]);
    }

    #[test]
    fn test_rrf_fuse_ordering_and_weight() {
        crate::isolate_test_data_dir();
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
        crate::isolate_test_data_dir();
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
        crate::isolate_test_data_dir();
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
        crate::isolate_test_data_dir();
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
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let chunk = "x".repeat(100);
        let pairs = vec![(chunk.as_str(), "ghost.rs")];
        let (tokens_full, _, pct) = compute_search_savings(dir.path(), &pairs);
        assert_eq!(tokens_full, 0);
        assert_eq!(pct, 0.0);
    }

    #[test]
    fn test_savings_empty_results() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let (tokens_full, tokens_returned, pct) = compute_search_savings(dir.path(), &[]);
        assert_eq!(tokens_full, 0);
        assert_eq!(tokens_returned, 0);
        assert_eq!(pct, 0.0);
    }

    #[test]
    fn test_savings_zero_pct_when_chunk_not_smaller_than_file() {
        crate::isolate_test_data_dir();
        // Returned chunk is bigger than the source file — no savings claimed.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tiny.rs"), "x".repeat(40)).unwrap();
        let chunk = "y".repeat(400);
        let pairs = vec![(chunk.as_str(), "tiny.rs")];
        let (tokens_full, tokens_returned, pct) = compute_search_savings(dir.path(), &pairs);
        assert_eq!(tokens_full, 10, "40 bytes / 4");
        assert_eq!(tokens_returned, 100, "400 bytes / 4");
        assert_eq!(pct, 0.0, "no savings when returned >= full");
    }

    #[test]
    fn test_record_real_search_savings_smoke() {
        crate::isolate_test_data_dir();
        // Fire-and-forget telemetry: must neither panic nor block the caller,
        // even for a file that doesn't exist on disk.
        let dir = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: format!("savings-fixture-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let results = vec![SearchResult {
            file_path: "a.rs".to_string(),
            chunk: "fn a() {}".to_string(),
            score: 0.5,
            line_start: 1,
            line_end: 1,
            community_id: None,
        }];
        record_real_search_savings(&project, "unit_test", &results);
        record_real_search_savings(&project, "unit_test", &[]);
    }
}

#[cfg(test)]
mod search_api_tests {
    use super::*;
    use crate::model::Project;

    fn fixture_project(name: &str, dir: &tempfile::TempDir) -> Project {
        Project {
            name: format!("search-{}-fixture-{}", name, std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    /// Insert a chunk and mirror it into the FTS index; returns the chunk id.
    fn insert_chunk(conn: &rusqlite::Connection, file: &str, text: &str) -> i64 {
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
             VALUES (?1, 1, 10, ?2, 0)",
            rusqlite::params![file, text],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        super::super::db::fts_replace_file(conn, file).unwrap();
        id
    }

    /// Little-endian f32 blob, matching the on-disk embedding encoding.
    fn le_blob(vals: &[f32]) -> Vec<u8> {
        vals.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn test_search_with_mode_missing_index_errors() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("noindex", &dir);
        let err = search_with_mode(&project, "anything", 5, SearchMode::Lexical).unwrap_err();
        assert!(
            matches!(err, IndexError::IndexNotFound(ref name) if *name == project.name),
            "expected IndexNotFound, got {err:?}"
        );
    }

    /// Lexical-only search over a real meta.db: no embeddings and no HNSW
    /// files needed. Exercises the full search_with_mode pipeline (FTS rank,
    /// fusion with an empty vector side, chunk hydration, top_k truncation).
    #[test]
    fn test_lexical_search_end_to_end() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("lexical", &dir);
        let conn = super::super::db::open_meta_db(&project).unwrap();

        insert_chunk(&conn, "a.rs", "fn alpha() { shared_token(); }");
        insert_chunk(&conn, "b.rs", "fn beta() { shared_token(); }");
        insert_chunk(&conn, "c.rs", "fn gamma() { shared_token(); }");
        insert_chunk(&conn, "d.rs", "fn unrelated() {}");
        drop(conn);

        // top_k truncates the fused list.
        let results = search_with_mode(&project, "shared_token", 2, SearchMode::Lexical).unwrap();
        assert_eq!(results.len(), 2, "top_k must cap the result count");
        for r in &results {
            assert!(r.chunk.contains("shared_token"));
            assert_eq!(r.line_start, 1);
            assert_eq!(r.line_end, 10);
            assert_eq!(
                r.score, 0.0,
                "lexical-only hits get score 0 without embeddings"
            );
            assert_eq!(r.community_id, None, "no clustering has run");
        }

        // A query with no usable tokens yields no results (not an error).
        let results = search_with_mode(&project, "?!", 5, SearchMode::Lexical).unwrap();
        assert!(results.is_empty());

        // An identifier that matches nothing yields no results.
        let results =
            search_with_mode(&project, "zz_missing_symbol", 5, SearchMode::Lexical).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_chunk_embedding_cosine_cases() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("cosine", &dir);
        let conn = super::super::db::open_meta_db(&project).unwrap();

        let id_same = insert_chunk(&conn, "x.rs", "unit x");
        let id_ortho = insert_chunk(&conn, "y.rs", "unit y");
        let id_neg = insert_chunk(&conn, "w.rs", "unit -x");
        let id_zero = insert_chunk(&conn, "z.rs", "zero vector");
        let id_short = insert_chunk(&conn, "s.rs", "wrong dims");
        let id_none = insert_chunk(&conn, "n.rs", "no embedding");

        let set = |id: i64, blob: Vec<u8>| {
            conn.execute(
                "UPDATE chunks SET embedding = ?1 WHERE id = ?2",
                rusqlite::params![blob, id],
            )
            .unwrap();
        };
        set(id_same, le_blob(&[1.0, 0.0, 0.0]));
        set(id_ortho, le_blob(&[0.0, 1.0, 0.0]));
        set(id_neg, le_blob(&[-1.0, 0.0, 0.0]));
        set(id_zero, le_blob(&[0.0, 0.0, 0.0]));
        set(id_short, le_blob(&[1.0, 0.0]));

        let q = [1.0f32, 0.0, 0.0];
        let same = chunk_embedding_cosine(&conn, id_same, &q).expect("same direction");
        assert!((same - 1.0).abs() < 1e-6, "identical vectors → 1.0");
        let ortho = chunk_embedding_cosine(&conn, id_ortho, &q).expect("orthogonal");
        assert!(ortho.abs() < 1e-6, "orthogonal vectors → 0.0");
        let neg = chunk_embedding_cosine(&conn, id_neg, &q).expect("opposite");
        assert!((neg + 1.0).abs() < 1e-6, "opposite vectors → -1.0");

        assert!(
            chunk_embedding_cosine(&conn, id_zero, &q).is_none(),
            "zero-norm embedding yields None"
        );
        assert!(
            chunk_embedding_cosine(&conn, id_short, &q).is_none(),
            "dimension mismatch yields None"
        );
        assert!(
            chunk_embedding_cosine(&conn, id_none, &q).is_none(),
            "chunk without a stored embedding yields None"
        );
        assert!(
            chunk_embedding_cosine(&conn, 999_999, &q).is_none(),
            "unknown chunk id yields None"
        );
    }

    #[test]
    fn test_invalidate_hnsw_cache_without_loaded_cache_is_noop() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("invalidate", &dir);
        // Must be safe whether or not the process-wide cache was ever
        // initialized, and must stay idempotent.
        invalidate_hnsw_cache(&project);
        invalidate_hnsw_cache(&project);
    }

    // ── Bounded HNSW cache ──────────────────────────────────

    /// Dump a tiny 8-point HNSW for `project` and map hnsw_id i → chunk
    /// id 100+i, mirroring what the indexer writes.
    fn write_tiny_index(project: &Project) -> Vec<Vec<f32>> {
        let embeddings: Vec<Vec<f32>> = (0..8)
            .map(|i| (0..8).map(|j| (((i * 8 + j) as f32) + 1.0).sin()).collect())
            .collect();
        let hnsw: Hnsw<f32, DistCosine> = Hnsw::new(
            HNSW_MAX_CONN,
            embeddings.len(),
            HNSW_MAX_LAYERS,
            50,
            DistCosine,
        );
        for (i, e) in embeddings.iter().enumerate() {
            hnsw.insert((e.as_slice(), i));
        }
        let dir = hnsw_dir(project);
        std::fs::create_dir_all(&dir).unwrap();
        hnsw.file_dump(&dir, "index").unwrap();

        let conn = super::super::db::open_meta_db(project).unwrap();
        for i in 0..8i64 {
            conn.execute(
                "INSERT OR REPLACE INTO chunk_order (hnsw_id, chunk_id) VALUES (?1, ?2)",
                rusqlite::params![i, 100 + i],
            )
            .unwrap();
        }
        embeddings
    }

    fn is_cached(project: &Project) -> bool {
        let key = hnsw_dir(project).to_string_lossy().to_string();
        hnsw_cache().lock().unwrap().contains_key(&key)
    }

    #[test]
    fn test_hnsw_graph_file_is_what_hnsw_rs_writes() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("graphfile", &dir);
        write_tiny_index(&project);
        assert!(
            hnsw_graph_file(&project).is_file(),
            "the version stamp must point at a file hnsw_rs actually dumps"
        );
    }

    #[test]
    fn test_with_hnsw_keep_caches_and_maps_ids() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("keep", &dir);
        let embeddings = write_tiny_index(&project);
        let conn = super::super::db::open_meta_db(&project).unwrap();

        let hits = with_hnsw(&project, &conn, CachePolicy::Keep, |c| {
            c.hnsw
                .search(&embeddings[3], 1, 16)
                .into_iter()
                .map(|n| c.chunk_order[n.d_id])
                .collect::<Vec<_>>()
        })
        .unwrap();
        assert_eq!(
            hits,
            vec![103],
            "nearest neighbour maps through chunk_order"
        );
        assert!(is_cached(&project), "Keep leaves the index resident");

        invalidate_hnsw_cache(&project);
        assert!(!is_cached(&project));
    }

    #[test]
    fn test_with_hnsw_transient_does_not_cache() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("transient", &dir);
        write_tiny_index(&project);
        let conn = super::super::db::open_meta_db(&project).unwrap();

        let points = with_hnsw(&project, &conn, CachePolicy::Transient, |c| {
            c.hnsw.get_nb_point()
        })
        .unwrap();
        assert_eq!(points, 8);
        assert!(
            !is_cached(&project),
            "a fan-out search must not pin the index in memory"
        );
    }

    #[test]
    fn test_with_hnsw_reloads_when_disk_index_is_newer() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("reload", &dir);
        write_tiny_index(&project);
        let conn = super::super::db::open_meta_db(&project).unwrap();
        with_hnsw(&project, &conn, CachePolicy::Keep, |_| ()).unwrap();

        // Pretend the cached copy predates the on-disk index (another
        // process re-indexed): the next search must reload, not reuse.
        let key = hnsw_dir(&project).to_string_lossy().to_string();
        hnsw_cache()
            .lock()
            .unwrap()
            .get_mut(&key)
            .unwrap()
            .loaded_mtime = 0.0;
        let stamp = with_hnsw(&project, &conn, CachePolicy::Keep, |c| c.loaded_mtime).unwrap();
        assert!(stamp > 0.0, "stale entry was replaced by a fresh load");
        invalidate_hnsw_cache(&project);
    }

    #[test]
    fn test_parse_capacity() {
        assert_eq!(parse_capacity(None), DEFAULT_HNSW_CACHE_CAPACITY);
        assert_eq!(parse_capacity(Some("5")), 5);
        assert_eq!(parse_capacity(Some(" 3 ")), 3);
        assert_eq!(parse_capacity(Some("0")), DEFAULT_HNSW_CACHE_CAPACITY);
        assert_eq!(parse_capacity(Some("lots")), DEFAULT_HNSW_CACHE_CAPACITY);
    }

    #[test]
    fn test_evict_lru_drops_oldest_first() {
        let t0 = Instant::now();
        let mut map: HashMap<String, Instant> = HashMap::new();
        map.insert("a".into(), t0);
        map.insert("b".into(), t0 + Duration::from_secs(2));
        map.insert("c".into(), t0 + Duration::from_secs(1));
        evict_lru(&mut map, 2, |t| *t);
        assert!(!map.contains_key("a"), "least recently used goes first");
        evict_lru(&mut map, 1, |t| *t);
        assert_eq!(map.keys().collect::<Vec<_>>(), vec!["b"]);
        evict_lru(&mut map, 5, |t| *t);
        assert_eq!(map.len(), 1, "under capacity is a no-op");
    }

    #[test]
    fn test_evict_idle_respects_ttl() {
        let now = Instant::now() + Duration::from_secs(600);
        let mut map: HashMap<String, Instant> = HashMap::new();
        map.insert("fresh".into(), now - Duration::from_secs(10));
        map.insert("idle".into(), now - Duration::from_secs(120));
        let dropped = evict_idle(&mut map, Duration::from_secs(60), now, |t| *t);
        assert_eq!(dropped, 1);
        assert!(map.contains_key("fresh") && !map.contains_key("idle"));
    }

    #[test]
    fn test_reap_idle_releases_stale_hnsw() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("reap", &dir);
        write_tiny_index(&project);
        let conn = super::super::db::open_meta_db(&project).unwrap();
        with_hnsw(&project, &conn, CachePolicy::Keep, |_| ()).unwrap();
        assert!(is_cached(&project));

        reap_idle(Instant::now() + HNSW_IDLE_TTL + Duration::from_secs(1));
        assert!(!is_cached(&project), "idle index is released by the reaper");
    }

    #[test]
    fn test_delete_index_releases_cached_graph() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("delete", &dir);
        write_tiny_index(&project);
        let conn = super::super::db::open_meta_db(&project).unwrap();
        with_hnsw(&project, &conn, CachePolicy::Keep, |_| ()).unwrap();
        drop(conn);
        assert!(is_cached(&project));

        super::super::stats::delete_index(&project).unwrap();
        assert!(
            !is_cached(&project),
            "a deleted index must not keep serving searches"
        );
        assert!(!index_exists(&project));
    }

    #[test]
    fn test_mmap_load_matches_heap_load() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("mmap", &dir);
        let embeddings = write_tiny_index(&project);
        let conn = super::super::db::open_meta_db(&project).unwrap();
        let neighbours = |mmap: bool| {
            let c = CachedHnsw::load(&project, &conn, mmap).unwrap();
            embeddings
                .iter()
                .map(|e| {
                    c.hnsw
                        .search(e, 3, 16)
                        .iter()
                        .map(|n| n.d_id)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            neighbours(true),
            neighbours(false),
            "memory-mapped vectors must give the same neighbours"
        );
    }
}
