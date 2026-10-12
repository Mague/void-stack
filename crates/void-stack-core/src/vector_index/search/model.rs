//! The embedding model: created on first use, shared by indexing and
//! search, and dropped after `MODEL_IDLE_TTL` without use (it is re-created
//! transparently on the next embed).

use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use crate::error::IndexError;

/// Cached embedding model — created on demand, dropped when idle.
static EMBEDDING_MODEL: Mutex<Option<ModelSlot>> = Mutex::new(None);

struct ModelSlot {
    model: fastembed::TextEmbedding,
    last_used: Instant,
}

/// The embedding model (and the ONNX arena it grew while indexing) is
/// dropped after this long without an embed call.
const MODEL_IDLE_TTL: Duration = Duration::from_secs(5 * 60);

/// Cap ONNX/OpenMP intra-op thread pools so fastembed doesn't spawn a
/// thread per logical CPU and blow past the indexer's dedicated rayon pool.
/// Runs once per process, before the first model is created (ONNX reads
/// these at init). `set_var` is unsafe under edition 2024 because it races
/// with concurrent getenv readers; the `Once` serializes it.
fn configure_onnx_threads() {
    static ONNX_ENV: Once = Once::new();
    ONNX_ENV.call_once(|| {
        let threads = crate::vector_index::indexer::indexing_rayon_threads().to_string();
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

    let cache_dir = crate::vector_index::stats::model_cache_dir();
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
        super::cache::ensure_reaper();
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

/// Drop the model if it has been idle for `MODEL_IDLE_TTL` (reaper pass).
pub(super) fn release_if_idle(now: Instant) {
    if let Ok(mut slot) = EMBEDDING_MODEL.lock()
        && slot
            .as_ref()
            .is_some_and(|m| now.saturating_duration_since(m.last_used) >= MODEL_IDLE_TTL)
    {
        *slot = None;
        tracing::debug!("released idle embedding model");
    }
}
