//! Semantic search: embedding model, HNSW cache, and search API.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::hnswio::HnswIo;
use hnsw_rs::prelude::*;
use serde::Serialize;

use super::chunker::CHUNK_LINES;
use super::stats::{file_mtime, hnsw_dir, index_exists};
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
}

// ── Public API ──────────────────────────────────────────────

/// Semantic search across the indexed codebase.
pub fn semantic_search(
    project: &Project,
    query: &str,
    top_k: usize,
) -> Result<Vec<SearchResult>, String> {
    if !index_exists(project) {
        return Err(format!(
            "No index found for project '{}'. Run `void index {}` first.",
            project.name, project.name
        ));
    }

    let conn = super::db::open_meta_db(project)?;

    // Load chunk order
    let chunk_order = super::db::load_chunk_order(&conn)?;

    // Embed query using cached model
    let query_emb = embed_texts(&[query.to_string()])?;
    if query_emb.is_empty() {
        return Err("Failed to generate query embedding".to_string());
    }

    // Search using cached HNSW index
    let cache_key = ensure_hnsw_cached(project)?;
    let hnsw_cache = HNSW_CACHE
        .get()
        .ok_or_else(|| "HNSW cache not initialized".to_string())?;
    let hnsw_map = hnsw_cache
        .lock()
        .map_err(|e| format!("HNSW cache lock poisoned: {}", e))?;
    let cached = hnsw_map
        .get(&cache_key)
        .ok_or_else(|| "HNSW index not in cache".to_string())?;

    let ef_search = top_k.max(HNSW_MAX_CONN);
    let neighbours = cached.hnsw.search(&query_emb[0], top_k, ef_search);

    let mut results = Vec::with_capacity(neighbours.len());
    for neighbour in &neighbours {
        let hnsw_id = neighbour.d_id;
        if let Some(chunk_id) = chunk_order.get(hnsw_id)
            && let Ok(chunk) = super::db::load_chunk_by_id(&conn, *chunk_id)
        {
            // hnsw_rs returns distance, convert to similarity for cosine
            let score = 1.0 - neighbour.distance;
            results.push(SearchResult {
                file_path: chunk.file_path,
                chunk: chunk.text,
                score,
                line_start: chunk.line_start,
                line_end: chunk.line_end,
            });
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Record search in stats
    let avg_chunk_lines = CHUNK_LINES;
    crate::stats::record_saving(crate::stats::TokenSavingsRecord {
        timestamp: Utc::now(),
        project: project.name.clone(),
        operation: "semantic_search".to_string(),
        lines_original: top_k * avg_chunk_lines,
        lines_filtered: top_k,
        savings_pct: if top_k > 0 {
            (1.0 - (1.0 / avg_chunk_lines as f32)) * 100.0
        } else {
            0.0
        },
    });

    Ok(results)
}

// ── Embedding model ─────────────────────────────────────────

/// Get or initialize the cached embedding model.
fn get_embedding_model() -> Result<&'static Mutex<fastembed::TextEmbedding>, String> {
    if let Some(m) = EMBEDDING_MODEL.get() {
        return Ok(m);
    }

    let cache_dir = super::stats::model_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);

    let options = fastembed::InitOptions::new(fastembed::EmbeddingModel::BGESmallENV15)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(true);

    let model = fastembed::TextEmbedding::try_new(options)
        .map_err(|e| format!("Model init error: {}", e))?;

    // Race-safe: if another thread initialized first, use theirs
    let _ = EMBEDDING_MODEL.set(Mutex::new(model));
    EMBEDDING_MODEL
        .get()
        .ok_or_else(|| "Embedding model not initialized".to_string())
}

/// Embed texts using the cached model.
pub(crate) fn embed_texts(texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let model_lock = get_embedding_model()?;
    let model = model_lock
        .lock()
        .map_err(|e| format!("Model lock poisoned: {}", e))?;
    model
        .embed(texts.to_vec(), None)
        .map_err(|e| format!("Embedding error: {}", e))
}

/// Ensure the HNSW index is loaded into cache for a project. Returns the cache key.
fn ensure_hnsw_cached(project: &Project) -> Result<String, String> {
    let cache = HNSW_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let hnsw_path = hnsw_dir(project);
    let key = hnsw_path.to_string_lossy().to_string();
    let data_file = hnsw_path.join("index_data.hnsw");
    let current_mtime = file_mtime(&data_file);

    let mut map = cache
        .lock()
        .map_err(|e| format!("HNSW cache lock poisoned: {}", e))?;

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
            .map_err(|e| format!("Failed to load HNSW index: {}", e))?;

        // Validate the index has points — an empty HNSW silently returns 0 results
        let nb_points = hnsw.get_nb_point();
        if nb_points == 0 {
            return Err(format!(
                "HNSW index at '{}' is empty (0 points). \
                 The index may be corrupted or still building. \
                 Run index_project_codebase with force=true to rebuild.",
                hnsw_path.display()
            ));
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
