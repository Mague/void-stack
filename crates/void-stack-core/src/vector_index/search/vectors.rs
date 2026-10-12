//! Exact vector search over int8-quantized embeddings.
//!
//! Replaces the HNSW graph. Measured on the indexes of a real workstation
//! (423 to 19,703 chunks, `examples/vector_bench.rs`): HNSW returned
//! 92–97 % of the true top 10, an int8 scan plus an exact f32 re-score of
//! the 40 best candidates returns 100 % in 0.04–1.5 ms per query — noise
//! next to the ~50 ms it takes to embed the query — while holding 4× less
//! memory than the f32 vectors (7.3 MB for the largest index vs ~37 MB of
//! HNSW graph + data) and needing no graph build, no files besides
//! `meta.db`, and no unsafe code.
//!
//! Vectors are quantized per row with a symmetric scale (`max |x| / 127`).
//! Scores from the int8 scan only pick candidates; the returned cosine is
//! always computed from the stored f32 embedding.

use rusqlite::Connection;

use crate::error::IndexError;

/// Candidates kept from the int8 scan for exact re-scoring: `k × 4`, at
/// least this many.
const MIN_SHORTLIST: usize = 40;

pub(crate) struct VectorIndex {
    ids: Vec<i64>,
    dim: usize,
    codes: Vec<i8>,
    scales: Vec<f32>,
}

impl VectorIndex {
    /// Read every stored embedding (`chunks.embedding`, little-endian f32)
    /// and quantize it. Rows whose dimension differs from the first one
    /// (a half-migrated index) are skipped.
    pub(crate) fn load(conn: &Connection) -> Result<Self, IndexError> {
        let mut stmt = conn
            .prepare("SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL ORDER BY id")
            .map_err(IndexError::from)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))
            .map_err(IndexError::from)?;
        let mut index = VectorIndex {
            ids: Vec::new(),
            dim: 0,
            codes: Vec::new(),
            scales: Vec::new(),
        };
        let mut row = Vec::new();
        for (id, blob) in rows.flatten() {
            decode_into(&blob, &mut row);
            index.push(id, &row);
        }
        Ok(index)
    }

    #[cfg(test)]
    pub(crate) fn from_vectors(rows: &[(i64, Vec<f32>)]) -> Self {
        let mut index = VectorIndex {
            ids: Vec::new(),
            dim: 0,
            codes: Vec::new(),
            scales: Vec::new(),
        };
        for (id, v) in rows {
            index.push(*id, v);
        }
        index
    }

    fn push(&mut self, id: i64, v: &[f32]) {
        if v.is_empty() {
            return;
        }
        if self.dim == 0 {
            self.dim = v.len();
        } else if v.len() != self.dim {
            return;
        }
        let max = v.iter().fold(0f32, |m, x| m.max(x.abs()));
        if max == 0.0 || !max.is_finite() {
            return;
        }
        self.ids.push(id);
        self.scales.push(max / 127.0);
        self.codes
            .extend(v.iter().map(|x| (x / max * 127.0).round() as i8));
    }

    pub(crate) fn len(&self) -> usize {
        self.ids.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Approximate memory held by the index.
    pub(crate) fn bytes(&self) -> usize {
        self.codes.len() + self.scales.len() * 4 + self.ids.len() * 8
    }

    /// Chunk ids of the `n` best rows by the int8 dot product. Every row is
    /// scaled by its own norm proxy, so this ranks by cosine up to
    /// quantization error.
    fn shortlist(&self, query: &[f32], n: usize) -> Vec<i64> {
        if self.is_empty() || query.len() != self.dim || n == 0 {
            return Vec::new();
        }
        let qmax = query.iter().fold(0f32, |m, x| m.max(x.abs()));
        if qmax == 0.0 || !qmax.is_finite() {
            return Vec::new();
        }
        let q: Vec<i32> = query
            .iter()
            .map(|x| (x / qmax * 127.0).round() as i32)
            .collect();
        let mut scored: Vec<(f32, usize)> = self
            .codes
            .chunks_exact(self.dim)
            .enumerate()
            .map(|(i, row)| {
                let dot: i32 = row.iter().zip(&q).map(|(a, b)| *a as i32 * b).sum();
                (dot as f32 * self.scales[i], i)
            })
            .collect();
        let n = n.min(scored.len());
        scored.select_nth_unstable_by(n - 1, |a, b| b.0.total_cmp(&a.0));
        scored.truncate(n);
        scored.into_iter().map(|(_, i)| self.ids[i]).collect()
    }

    /// The `k` nearest chunks as `(chunk_id, cosine)`, best first: an int8
    /// scan picks `max(4k, 40)` candidates and their stored f32 embeddings
    /// decide the final order and score.
    pub(crate) fn search(
        &self,
        conn: &Connection,
        query: &[f32],
        k: usize,
    ) -> Result<Vec<(i64, f32)>, IndexError> {
        let candidates = self.shortlist(query, (k * 4).max(MIN_SHORTLIST));
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let qnorm = norm(query);
        let placeholders = vec!["?"; candidates.len()].join(",");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id, embedding FROM chunks WHERE id IN ({placeholders})"
            ))
            .map_err(IndexError::from)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(candidates.iter()), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<Vec<u8>>>(1)?))
            })
            .map_err(IndexError::from)?;
        let mut row = Vec::new();
        let mut scored: Vec<(i64, f32)> = Vec::with_capacity(candidates.len());
        for (id, blob) in rows.flatten() {
            let Some(blob) = blob else { continue };
            decode_into(&blob, &mut row);
            if row.len() != query.len() {
                continue;
            }
            let n = norm(&row);
            if n == 0.0 || qnorm == 0.0 {
                continue;
            }
            let dot: f32 = row.iter().zip(query).map(|(a, b)| a * b).sum();
            scored.push((id, dot / (n * qnorm)));
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scored.truncate(k);
        Ok(scored)
    }
}

fn decode_into(blob: &[u8], out: &mut Vec<f32>) {
    out.clear();
    out.extend(
        blob.as_chunks::<4>()
            .0
            .iter()
            .map(|&b| f32::from_le_bytes(b)),
    );
}

fn norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_with(rows: &[(i64, Vec<f32>)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE chunks (id INTEGER PRIMARY KEY, embedding BLOB)")
            .unwrap();
        for (id, v) in rows {
            let blob: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
            conn.execute(
                "INSERT INTO chunks (id, embedding) VALUES (?1, ?2)",
                rusqlite::params![id, blob],
            )
            .unwrap();
        }
        conn
    }

    /// Deterministic pseudo-random unit-ish vectors.
    fn vectors(n: usize, dim: usize) -> Vec<(i64, Vec<f32>)> {
        let mut state = 0x2545_f491_u64;
        (0..n)
            .map(|i| {
                let v = (0..dim)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        (state % 2000) as f32 / 1000.0 - 1.0
                    })
                    .collect();
                (i as i64 + 1, v)
            })
            .collect()
    }

    fn exact(rows: &[(i64, Vec<f32>)], q: &[f32], k: usize) -> Vec<i64> {
        let mut s: Vec<(f32, i64)> = rows
            .iter()
            .map(|(id, v)| {
                let dot: f32 = v.iter().zip(q).map(|(a, b)| a * b).sum();
                (dot / (norm(v) * norm(q)), *id)
            })
            .collect();
        s.sort_by(|a, b| b.0.total_cmp(&a.0));
        s.into_iter().take(k).map(|(_, id)| id).collect()
    }

    #[test]
    fn search_matches_exact_cosine_top_k() {
        let rows = vectors(500, 64);
        let conn = conn_with(&rows);
        let index = VectorIndex::load(&conn).unwrap();
        assert_eq!(index.len(), 500);
        for (_, q) in rows.iter().step_by(37) {
            let got: Vec<i64> = index
                .search(&conn, q, 10)
                .unwrap()
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            assert_eq!(got, exact(&rows, q, 10), "re-scored top 10 is exact");
        }
    }

    #[test]
    fn scores_are_true_cosines() {
        let rows = vec![
            (1, vec![1.0, 0.0, 0.0]),
            (2, vec![0.0, 2.0, 0.0]),
            (3, vec![-3.0, 0.0, 0.0]),
        ];
        let conn = conn_with(&rows);
        let index = VectorIndex::load(&conn).unwrap();
        let hits = index.search(&conn, &[2.0, 0.0, 0.0], 3).unwrap();
        assert_eq!(hits[0].0, 1);
        assert!((hits[0].1 - 1.0).abs() < 1e-6);
        assert!(hits[1].1.abs() < 1e-6, "orthogonal → 0");
        assert!((hits[2].1 + 1.0).abs() < 1e-6, "opposite → -1");
    }

    #[test]
    fn skips_bad_rows_and_bad_queries() {
        let rows = vec![
            (1, vec![1.0, 0.0]),
            (2, vec![0.0, 0.0]),      // zero vector
            (3, vec![1.0, 1.0, 1.0]), // wrong dimension
            (4, vec![0.5, 0.5]),
        ];
        let index = VectorIndex::from_vectors(&rows);
        assert_eq!(index.len(), 2);
        let conn = conn_with(&rows);
        assert!(index.search(&conn, &[0.0, 0.0], 5).unwrap().is_empty());
        assert!(index.search(&conn, &[1.0, 0.0, 0.0], 5).unwrap().is_empty());
        assert_eq!(index.search(&conn, &[1.0, 0.0], 5).unwrap().len(), 2);
    }

    #[test]
    fn int8_codes_take_a_quarter_of_f32() {
        let rows = vectors(100, 384);
        let index = VectorIndex::from_vectors(&rows);
        assert_eq!(index.codes.len(), 100 * 384);
        assert!(index.bytes() < 100 * 384 * 4 / 3);
    }
}
