//! Semantic search: hybrid (vector + BM25) ranking over a project's index.

mod cache;
mod model;
mod rank;
mod savings;
mod vectors;

use std::collections::HashMap;

use serde::Serialize;

use super::stats::index_exists;
use crate::error::IndexError;
use crate::model::Project;

use cache::with_index;
pub(crate) use cache::{CachePolicy, invalidate_index_cache};
pub(crate) use model::embed_texts;
#[cfg(test)]
pub(crate) use model::{FAKE_EMBEDDINGS, fake_embedding as fake_embedding_pub};
pub(crate) use rank::is_import_line as is_import_line_pub;
pub use rank::{ChunkKind, chunk_kind};
use savings::record_real_search_savings;

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
pub(crate) const RRF_K: f32 = 60.0;
/// Lexical weight multiplier for exact-identifier queries.
pub(crate) const IDENTIFIER_LEXICAL_BOOST: f32 = 2.0;

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
    search_with_policy(project, query, None, top_k, mode, CachePolicy::Keep, true)
}

/// [`search_with_mode`] for callers that render the hits themselves
/// (compact previews, session dedupe): records nothing — the caller
/// reports what it actually sent with [`record_sent_savings`].
pub fn search_unrecorded(
    project: &Project,
    query: &str,
    top_k: usize,
    mode: SearchMode,
) -> Result<Vec<SearchResult>, IndexError> {
    search_with_policy(project, query, None, top_k, mode, CachePolicy::Keep, false)
}

/// Record token savings using the size of what was really sent to the
/// agent (`sent_chars`), against the full size of the files the hits came
/// from — the honest version of the bookkeeping: compact output counts as
/// compact, a hit sent earlier in the session counts as nothing.
pub fn record_sent_savings(
    project: &Project,
    operation: &str,
    results: &[SearchResult],
    sent_chars: usize,
) {
    savings::record_sent(project, operation, results, sent_chars);
}

/// Hybrid search that does not keep `project`'s vector index resident —
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
        true,
    )
}

fn search_with_policy(
    project: &Project,
    query: &str,
    precomputed_emb: Option<&[f32]>,
    top_k: usize,
    mode: SearchMode,
    policy: CachePolicy,
    record: bool,
) -> Result<Vec<SearchResult>, IndexError> {
    if !index_exists(project) {
        return Err(IndexError::IndexNotFound(project.name.clone()));
    }
    let conn = super::db::open_meta_db(project)?;
    // Over-fetch: reranking drops import-only blocks and caps hits per
    // file, so the fused list must hold more than top_k candidates.
    let fetch = (top_k * 4).max(20);

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
    let lexical_scored: Vec<(i64, f32)> = if mode != SearchMode::Vector {
        lexical_rank_scored(&conn, query, fetch).unwrap_or_default()
    } else {
        Vec::new()
    };
    let lexical_ranked: Vec<i64> = lexical_scored.iter().map(|(id, _)| *id).collect();
    // Lexical mode never loads the model, so there is no cosine to show.
    // Score its hits by BM25 relative to the best hit (best = 1.00) — before
    // this every lexical hit printed score 0.00.
    let best_bm25 = lexical_scored
        .iter()
        .map(|(_, b)| *b)
        .fold(0.0_f32, f32::min);
    let bm25_norm: HashMap<i64, f32> = lexical_scored
        .iter()
        .map(|(id, b)| {
            let s = if best_bm25 < 0.0 {
                (b / best_bm25).clamp(0.0, 1.0)
            } else {
                1.0
            };
            (*id, s)
        })
        .collect();

    let lexical_weight = if is_identifier_query(query) {
        IDENTIFIER_LEXICAL_BOOST
    } else {
        1.0
    };
    let vector_ids: Vec<i64> = vector_ranked.iter().map(|(id, _)| *id).collect();
    let fused = rrf_fuse_scored(&vector_ids, &lexical_ranked, RRF_K, lexical_weight);

    // Rerank: weight by chunk kind (tests, docs, changelogs count less;
    // import-only blocks are dropped unless asked for), then diversify.
    let intent = rank::query_intent(query);
    let mut loaded: HashMap<i64, super::chunker::Chunk> = HashMap::new();
    let mut cands = Vec::with_capacity(fused.len());
    for (id, rrf) in fused {
        let Ok(chunk) = super::db::load_chunk_by_id(&conn, id) else {
            continue;
        };
        let kind = rank::chunk_kind(&chunk.file_path, &chunk.text);
        let Some(w) = rank::kind_weight(kind, intent) else {
            continue;
        };
        cands.push(rank::Candidate {
            id,
            file: chunk.file_path.clone(),
            score: rrf * w,
        });
        loaded.insert(id, chunk);
    }
    let top = rank::diversify(cands, top_k);
    let top_ids: Vec<i64> = top.iter().map(|c| c.id).collect();

    let vector_scores: HashMap<i64, f32> = vector_ranked.into_iter().collect();
    // Only the communities of the chunks we return — not the whole table.
    let communities = super::cluster::load_communities_for(&conn, &top_ids).unwrap_or_default();

    let mut results: Vec<SearchResult> = Vec::with_capacity(top_k);
    for chunk_id in top_ids {
        let Some(chunk) = loaded.remove(&chunk_id) else {
            continue;
        };
        // Cosine score: from the vector result when present, otherwise from
        // the stored embedding (lexical-only hits in hybrid mode). Lexical
        // mode has no query embedding: relative BM25 instead.
        let score = match vector_scores.get(&chunk_id) {
            Some(s) => *s,
            None => query_emb
                .as_deref()
                .and_then(|qe| chunk_embedding_cosine(&conn, chunk_id, qe))
                .or_else(|| {
                    bm25_norm
                        .get(&chunk_id)
                        .copied()
                        .filter(|_| mode == SearchMode::Lexical)
                })
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
    if record {
        record_real_search_savings(project, "semantic_search", &results);
    }

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

/// Vector side: nearest chunks as ranked (chunk_id, cosine) pairs.
pub(crate) fn vector_rank(
    project: &Project,
    conn: &rusqlite::Connection,
    query_emb: &[f32],
    fetch: usize,
    policy: CachePolicy,
) -> Result<Vec<(i64, f32)>, IndexError> {
    with_index(project, conn, policy, |cached| {
        cached.index.search(conn, query_emb, fetch)
    })?
}

/// Lexical side: BM25-ranked chunk ids from the FTS5 index.
// Production search uses the scored variant; the offline bench and tests
// use this one.
#[cfg_attr(not(any(test, feature = "bench")), allow(dead_code))]
pub(crate) fn lexical_rank(
    conn: &rusqlite::Connection,
    query: &str,
    fetch: usize,
) -> Result<Vec<i64>, IndexError> {
    Ok(lexical_rank_scored(conn, query, fetch)?
        .into_iter()
        .map(|(id, _)| id)
        .collect())
}

/// [`lexical_rank`] with each hit's BM25 (FTS5 convention: negative,
/// lower is better).
pub(crate) fn lexical_rank_scored(
    conn: &rusqlite::Connection,
    query: &str,
    fetch: usize,
) -> Result<Vec<(i64, f32)>, IndexError> {
    let Some(match_q) = build_fts_query(query) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn
        .prepare(
            "SELECT rowid, bm25(chunks_fts) AS s FROM chunks_fts WHERE chunks_fts MATCH ?1 \
             ORDER BY s LIMIT ?2",
        )
        .map_err(IndexError::from)?;
    let rows = stmt
        .query_map(rusqlite::params![match_q, fetch as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)? as f32))
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
#[cfg_attr(not(any(test, feature = "bench")), allow(dead_code))]
pub(crate) fn rrf_fuse(
    vector_ids: &[i64],
    lexical_ids: &[i64],
    k: f32,
    lexical_weight: f32,
) -> Vec<i64> {
    rrf_fuse_scored(vector_ids, lexical_ids, k, lexical_weight)
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

/// [`rrf_fuse`] keeping the fused score, for reranking.
pub(crate) fn rrf_fuse_scored(
    vector_ids: &[i64],
    lexical_ids: &[i64],
    k: f32,
    lexical_weight: f32,
) -> Vec<(i64, f32)> {
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
    out
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

    /// Lexical-only search over a real meta.db: no embeddings and no vector
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
        assert!(
            (results[0].score - 1.0).abs() < 1e-6,
            "best lexical hit scores 1.00"
        );
        for r in &results {
            assert!(r.chunk.contains("shared_token"));
            assert_eq!(r.line_start, 1);
            assert_eq!(r.line_end, 10);
            assert!(
                r.score > 0.0 && r.score <= 1.0,
                "lexical hits get a relative BM25 score, got {}",
                r.score
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
}
