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

/// Semantic search across the indexed codebase.
pub fn semantic_search(
    project: &Project,
    query: &str,
    top_k: usize,
) -> Result<Vec<SearchResult>, IndexError> {
    if !index_exists(project) {
        return Err(IndexError::IndexNotFound(project.name.clone()));
    }

    let conn = super::db::open_meta_db(project)?;

    // Load chunk order
    let chunk_order = super::db::load_chunk_order(&conn)?;

    // Embed query using cached model
    let query_emb = embed_texts(&[query.to_string()])?;
    if query_emb.is_empty() {
        return Err(IndexError::EmbeddingFailed(
            "failed to generate query embedding".to_string(),
        ));
    }

    // Search using cached HNSW index
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

    let ef_search = top_k.max(HNSW_MAX_CONN);
    let neighbours = cached.hnsw.search(&query_emb[0], top_k, ef_search);

    // Optional: JOIN with communities table if cluster_project has run.
    let communities = super::cluster::load_communities(&conn).unwrap_or_default();

    let mut results = Vec::with_capacity(neighbours.len());
    for neighbour in &neighbours {
        let hnsw_id = neighbour.d_id;
        if let Some(chunk_id) = chunk_order.get(hnsw_id)
            && let Ok(chunk) = super::db::load_chunk_by_id(&conn, *chunk_id)
        {
            // hnsw_rs returns distance, convert to similarity for cosine
            let score = 1.0 - neighbour.distance;
            let community_id = communities.get(chunk_id).copied();
            results.push(SearchResult {
                file_path: chunk.file_path,
                chunk: chunk.text,
                score,
                line_start: chunk.line_start,
                line_end: chunk.line_end,
                community_id,
            });
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Record *real* savings: tokens in the returned chunks vs the full
    // size of every unique file we drew them from. Approximates tokens
    // as bytes/4 (good enough for relative savings tracking — the absolute
    // token count varies with the tokenizer, but the ratio doesn't).
    record_real_search_savings(project, "semantic_search", &results);

    Ok(results)
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
pub(crate) fn record_real_search_savings(
    project: &Project,
    operation: &str,
    results: &[SearchResult],
) {
    let project_root = std::path::Path::new(&project.path);
    let pairs: Vec<(&str, &str)> = results
        .iter()
        .map(|r| (r.chunk.as_str(), r.file_path.as_str()))
        .collect();
    let (tokens_full, tokens_returned, savings_pct) = compute_search_savings(project_root, &pairs);

    crate::stats::record_saving(crate::stats::TokenSavingsRecord {
        timestamp: Utc::now(),
        project: project.name.clone(),
        operation: operation.to_string(),
        lines_original: tokens_full,
        lines_filtered: tokens_returned,
        savings_pct,
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
