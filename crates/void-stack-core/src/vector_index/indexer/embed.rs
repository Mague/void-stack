//! Embedding chunks in memory-friendly batches.

use crate::error::IndexError;
use crate::vector_index::chunker::Chunk;
use crate::vector_index::search::embed_texts;

/// Embed chunk texts in batches of similar length and return the vectors
/// in the original chunk order.
///
/// fastembed pads every batch to its longest text, so mixing a 20-token
/// chunk with a 512-token one wastes almost all of the short one's compute
/// (and memory). Sorting by length first makes each batch nearly uniform.
pub(super) fn embed_sorted_by_length(
    chunks: &[Chunk],
    batch_size: usize,
) -> Result<Vec<Vec<f32>>, IndexError> {
    let order = length_order(chunks.iter().map(|c| c.text.len()));
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); chunks.len()];
    for batch in order.chunks(batch_size.max(1)) {
        let texts: Vec<String> = batch.iter().map(|&i| chunks[i].text.clone()).collect();
        let embs = embed_texts(&texts)?;
        for (&i, emb) in batch.iter().zip(embs) {
            out[i] = emb;
        }
        // Yield between batches — lets services and OS breathe.
        // ONNX is already multi-threaded internally; no need to rush.
        std::thread::sleep(std::time::Duration::from_millis(EMBED_BATCH_YIELD_MS));
    }
    Ok(out)
}

/// Indices sorted by ascending length (stable, so equal lengths keep order).
pub(super) fn length_order(lengths: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut idx: Vec<(usize, usize)> = lengths.enumerate().collect();
    idx.sort_by_key(|&(_, len)| len);
    idx.into_iter().map(|(i, _)| i).collect()
}

/// Default chunks embedded per ONNX call. Peak memory of a BERT-style
/// forward pass grows with batch × seq_len² (attention scores), and ONNX's
/// arena keeps that high-water mark while the session lives. Measured on a
/// 3k-chunk project (length-sorted batches, arena on): 64 → 4.2 GB peak,
/// 16 → 1.2 GB / 105 s, 8 → 0.66 GB / 108 s, 4 → 0.52 GB / 119 s.
/// Override with `VOID_STACK_EMBED_BATCH=<n>` (1–256).
pub(super) const DEFAULT_EMBED_BATCH_SIZE: usize = 8;

pub(super) fn embed_batch_size() -> usize {
    parse_embed_batch(std::env::var("VOID_STACK_EMBED_BATCH").ok().as_deref())
}

pub(super) fn parse_embed_batch(raw: Option<&str>) -> usize {
    raw.and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| (1..=256).contains(n))
        .unwrap_or(DEFAULT_EMBED_BATCH_SIZE)
}

/// Yield window between embedding batches so other processes (services,
/// editors, OS) can breathe. ONNX is already multi-threaded internally.
const EMBED_BATCH_YIELD_MS: u64 = 50;
