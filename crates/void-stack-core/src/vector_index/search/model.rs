//! The embedding model: created on first use, shared by indexing and
//! search, and dropped after `MODEL_IDLE_TTL` without use (it is re-created
//! transparently on the next embed).

use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use crate::error::IndexError;

/// Cached CPU embedding model — created on demand, dropped when idle.
/// Searches always embed their query here: one short text costs a few
/// milliseconds on the CPU, less than handing it to a GPU.
static EMBEDDING_MODEL: Mutex<Option<ModelSlot>> = Mutex::new(None);

/// Cached GPU embedding model, used only for indexing (thousands of
/// chunks, where the GPU pays off). `None` until the first index run that
/// asks for it; [`GPU_UNAVAILABLE`] remembers a failed attempt so it is not
/// retried for every batch.
static GPU_MODEL: Mutex<Option<ModelSlot>> = Mutex::new(None);
static GPU_UNAVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct ModelSlot {
    model: fastembed::TextEmbedding,
    last_used: Instant,
}

/// Where index embeddings are computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedDevice {
    Cpu,
    /// DirectML (Windows; any DirectX 12 GPU: NVIDIA, AMD, Intel).
    DirectMl,
}

impl EmbedDevice {
    pub fn is_gpu(self) -> bool {
        !matches!(self, EmbedDevice::Cpu)
    }

    pub fn label(self) -> &'static str {
        match self {
            EmbedDevice::Cpu => "CPU",
            EmbedDevice::DirectMl => "GPU (DirectML)",
        }
    }
}

/// `VOID_STACK_EMBED_DEVICE=cpu` keeps indexing on the CPU; anything else
/// (or unset) tries the GPU first where this build supports one.
pub(crate) fn gpu_allowed(raw: Option<&str>) -> bool {
    !raw.is_some_and(|v| v.trim().eq_ignore_ascii_case("cpu"))
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

fn model_options(
    providers: Vec<ort::execution_providers::ExecutionProviderDispatch>,
) -> (fastembed::InitOptions, std::path::PathBuf) {
    configure_onnx_threads();
    let cache_dir = crate::vector_index::stats::model_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);
    let options = fastembed::InitOptions::new(fastembed::EmbeddingModel::BGESmallENV15)
        .with_cache_dir(cache_dir.clone())
        .with_show_download_progress(true)
        .with_execution_providers(providers);
    (options, cache_dir)
}

/// The CPU execution provider, with ONNX's memory arena unless
/// `VOID_STACK_ORT_ARENA=0`.
fn cpu_provider() -> ort::execution_providers::ExecutionProviderDispatch {
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
    cpu.build()
}

/// Create the GPU embedding model, if this build and machine have one.
/// The CPU provider stays registered behind it for any operator the GPU
/// provider doesn't implement.
#[cfg(windows)]
fn create_gpu_model() -> Option<(fastembed::TextEmbedding, EmbedDevice)> {
    let dml = ort::execution_providers::DirectMLExecutionProvider::default()
        .build()
        .error_on_failure();
    let (options, _) = model_options(vec![dml, cpu_provider()]);
    match fastembed::TextEmbedding::try_new(options) {
        Ok(model) => Some((model, EmbedDevice::DirectMl)),
        Err(e) => {
            tracing::warn!("DirectML unavailable, indexing on the CPU: {e}");
            None
        }
    }
}

#[cfg(not(windows))]
fn create_gpu_model() -> Option<(fastembed::TextEmbedding, EmbedDevice)> {
    None
}

/// Create the CPU embedding model (downloads it on first use).
fn create_embedding_model() -> Result<fastembed::TextEmbedding, IndexError> {
    let (options, cache_dir) = model_options(vec![cpu_provider()]);
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
    #[cfg(test)]
    if FAKE_EMBEDDINGS.with(|f| f.get()) {
        return Ok(texts.iter().map(|t| fake_embedding(t)).collect());
    }
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

#[cfg(test)]
thread_local! {
    /// Tests that run the whole indexer without the ONNX model set this on
    /// their thread: embeddings become a deterministic bag-of-words hash
    /// (same text → same vector; shared words → closer vectors).
    pub(crate) static FAKE_EMBEDDINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn fake_embedding(text: &str) -> Vec<f32> {
    use sha2::{Digest, Sha256};
    let mut v = vec![0.0f32; 384];
    for word in text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
    {
        let h = Sha256::digest(word.to_lowercase().as_bytes());
        let i = u16::from_le_bytes([h[0], h[1]]) as usize % 384;
        v[i] += if h[2] & 1 == 0 { 1.0 } else { -1.0 };
    }
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    v.into_iter().map(|x| x / n).collect()
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
    if let Ok(mut slot) = GPU_MODEL.lock()
        && slot
            .as_ref()
            .is_some_and(|m| now.saturating_duration_since(m.last_used) >= MODEL_IDLE_TTL)
    {
        *slot = None;
        tracing::debug!("released idle GPU embedding model");
    }
}

/// The device the next index run embeds on: loads the GPU model the first
/// time it's asked (unless `VOID_STACK_EMBED_DEVICE=cpu` or a previous
/// attempt failed) and falls back to the CPU.
pub(crate) fn index_embed_device() -> EmbedDevice {
    #[cfg(test)]
    if FAKE_EMBEDDINGS.with(|f| f.get()) {
        return EmbedDevice::Cpu;
    }
    use std::sync::atomic::Ordering;
    if !gpu_allowed(std::env::var("VOID_STACK_EMBED_DEVICE").ok().as_deref())
        || GPU_UNAVAILABLE.load(Ordering::Relaxed)
    {
        return EmbedDevice::Cpu;
    }
    let Ok(mut slot) = GPU_MODEL.lock() else {
        return EmbedDevice::Cpu;
    };
    if slot.is_none() {
        match create_gpu_model() {
            Some((model, device)) => {
                tracing::info!("indexing embeddings on {}", device.label());
                *slot = Some(ModelSlot {
                    model,
                    last_used: Instant::now(),
                });
                super::cache::ensure_reaper();
                GPU_DEVICE.store(device as u8, Ordering::Relaxed);
            }
            None => {
                GPU_UNAVAILABLE.store(true, Ordering::Relaxed);
                return EmbedDevice::Cpu;
            }
        }
    }
    match GPU_DEVICE.load(Ordering::Relaxed) {
        x if x == EmbedDevice::DirectMl as u8 => EmbedDevice::DirectMl,
        _ => EmbedDevice::Cpu,
    }
}

static GPU_DEVICE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Device of the last index run in this process that embedded anything
/// (0 = none yet), for the CLI's summary line.
static LAST_INDEX_DEVICE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub(crate) fn note_index_device(device: EmbedDevice) {
    LAST_INDEX_DEVICE.store(device as u8 + 1, std::sync::atomic::Ordering::Relaxed);
}

/// Where the last index run in this process computed its embeddings, or
/// `None` when no run embedded anything (nothing had changed).
pub fn last_index_device() -> Option<EmbedDevice> {
    match LAST_INDEX_DEVICE.load(std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        x if x == EmbedDevice::DirectMl as u8 + 1 => Some(EmbedDevice::DirectMl),
        _ => Some(EmbedDevice::Cpu),
    }
}

/// Embed chunks for the index on `device` (from [`index_embed_device`]).
/// A GPU error mid-run (driver reset, device lost) drops the GPU model and
/// finishes the batch on the CPU.
pub(crate) fn embed_for_index(
    texts: &[String],
    device: EmbedDevice,
) -> Result<Vec<Vec<f32>>, IndexError> {
    if !device.is_gpu() {
        return embed_texts(texts);
    }
    let result = {
        let mut slot = GPU_MODEL
            .lock()
            .map_err(|e| IndexError::EmbeddingFailed(format!("model lock poisoned: {}", e)))?;
        match slot.as_mut() {
            Some(entry) => {
                entry.last_used = Instant::now();
                let r = entry.model.embed(texts.to_vec(), None);
                if r.is_err() {
                    *slot = None;
                }
                r
            }
            None => return embed_texts(texts),
        }
    };
    match result {
        Ok(v) => Ok(v),
        Err(e) => {
            tracing::warn!("GPU embedding failed, continuing on the CPU: {e}");
            GPU_UNAVAILABLE.store(true, std::sync::atomic::Ordering::Relaxed);
            embed_texts(texts)
        }
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;

    #[test]
    fn test_gpu_allowed_unless_cpu_is_asked_for() {
        assert!(gpu_allowed(None));
        assert!(gpu_allowed(Some("gpu")));
        assert!(gpu_allowed(Some("auto")));
        assert!(!gpu_allowed(Some("cpu")));
        assert!(!gpu_allowed(Some(" CPU ")));
    }

    #[test]
    fn test_device_labels() {
        assert!(!EmbedDevice::Cpu.is_gpu());
        assert!(EmbedDevice::DirectMl.is_gpu());
        assert_eq!(EmbedDevice::DirectMl.label(), "GPU (DirectML)");
    }
}
