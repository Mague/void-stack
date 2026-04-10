//! SQLite metadata database: chunks, embeddings, chunk order.

use rusqlite::Connection;

use super::chunker::Chunk;
use super::stats::meta_db_path;
use crate::model::Project;

pub(crate) fn open_meta_db(project: &Project) -> Result<Connection, String> {
    let path = meta_db_path(project);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            text TEXT NOT NULL,
            mtime REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chunk_order (
            hnsw_id INTEGER PRIMARY KEY,
            chunk_id INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS stats (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);",
    )
    .map_err(|e| e.to_string())?;

    // Migration: add embedding column if not present
    let has_embedding: bool = conn
        .prepare("PRAGMA table_info(chunks)")
        .and_then(|mut stmt| {
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .flatten()
                .collect();
            Ok(rows.iter().any(|name| name == "embedding"))
        })
        .unwrap_or(false);

    if !has_embedding {
        conn.execute_batch("ALTER TABLE chunks ADD COLUMN embedding BLOB")
            .map_err(|e| e.to_string())?;
    }

    Ok(conn)
}

pub(crate) fn load_file_timestamps(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, f64>, String> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT file_path, MAX(mtime) FROM chunks GROUP BY file_path")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut map = std::collections::HashMap::new();
    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }
    Ok(map)
}

/// Load chunks with their cached embeddings for unchanged files.
/// Uses batched IN queries (max 999 params per SQLite limit).
pub(crate) fn load_chunks_with_embeddings(
    conn: &Connection,
    files: &[String],
) -> Result<Vec<(Chunk, Vec<f32>)>, String> {
    let mut results = Vec::new();
    if files.is_empty() {
        return Ok(results);
    }

    for batch in files.chunks(900) {
        let placeholders: String = batch
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT file_path, line_start, line_end, text, embedding FROM chunks WHERE file_path IN ({}) ORDER BY id",
            placeholders
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(rusqlite::params_from_iter(batch.iter()), |row| {
                let chunk = Chunk {
                    file_path: row.get(0)?,
                    text: row.get(3)?,
                    line_start: row.get::<_, i64>(1)? as usize,
                    line_end: row.get::<_, i64>(2)? as usize,
                };
                let embedding_blob: Option<Vec<u8>> = row.get(4)?;
                Ok((chunk, embedding_blob))
            })
            .map_err(|e| e.to_string())?;

        for row in rows.flatten() {
            let (chunk, embedding_blob) = row;
            if let Some(blob) = embedding_blob {
                let embedding = bytes_to_f32_vec(&blob);
                if !embedding.is_empty() {
                    results.push((chunk, embedding));
                }
            }
            // Skip chunks without embeddings — they'll be re-embedded
        }
    }

    Ok(results)
}

/// Save embeddings to the chunks table for newly indexed chunks.
pub(crate) fn save_embeddings(
    conn: &Connection,
    chunks: &[Chunk],
    embeddings: &[Vec<f32>],
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut stmt = tx
        .prepare(
            "UPDATE chunks SET embedding = ?1 WHERE file_path = ?2 AND line_start = ?3 AND line_end = ?4",
        )
        .map_err(|e| e.to_string())?;

    for (chunk, emb) in chunks.iter().zip(embeddings.iter()) {
        let blob = f32_vec_to_bytes(emb);
        let _ = stmt.execute(rusqlite::params![
            blob,
            chunk.file_path,
            chunk.line_start as i64,
            chunk.line_end as i64,
        ]);
    }

    drop(stmt);
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn save_chunk_order(conn: &Connection, chunks: &[Chunk]) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    tx.execute("DELETE FROM chunk_order", [])
        .map_err(|e| e.to_string())?;

    let mut stmt = tx
        .prepare("SELECT id FROM chunks WHERE file_path = ?1 AND line_start = ?2 AND line_end = ?3 LIMIT 1")
        .map_err(|e| e.to_string())?;

    let mut insert_stmt = tx
        .prepare("INSERT INTO chunk_order (hnsw_id, chunk_id) VALUES (?1, ?2)")
        .map_err(|e| e.to_string())?;

    for (hnsw_id, chunk) in chunks.iter().enumerate() {
        if let Ok(chunk_id) = stmt.query_row(
            rusqlite::params![
                chunk.file_path,
                chunk.line_start as i64,
                chunk.line_end as i64
            ],
            |row| row.get::<_, i64>(0),
        ) {
            let _ = insert_stmt.execute(rusqlite::params![hnsw_id as i64, chunk_id]);
        }
    }

    drop(stmt);
    drop(insert_stmt);
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn load_chunk_order(conn: &Connection) -> Result<Vec<i64>, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(hnsw_id), -1) FROM chunk_order",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    if count < 0 {
        return Ok(vec![]);
    }

    let mut order = vec![0i64; (count + 1) as usize];
    let mut stmt = conn
        .prepare("SELECT hnsw_id, chunk_id FROM chunk_order")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)? as usize, row.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        if row.0 < order.len() {
            order[row.0] = row.1;
        }
    }
    Ok(order)
}

pub(crate) fn load_chunk_by_id(conn: &Connection, chunk_id: i64) -> Result<Chunk, String> {
    conn.query_row(
        "SELECT file_path, line_start, line_end, text FROM chunks WHERE id = ?1",
        [chunk_id],
        |row| {
            Ok(Chunk {
                file_path: row.get(0)?,
                text: row.get(3)?,
                line_start: row.get::<_, i64>(1)? as usize,
                line_end: row.get::<_, i64>(2)? as usize,
            })
        },
    )
    .map_err(|e| e.to_string())
}

// ── Serialization helpers ──────────────────────────────────

fn f32_vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &val in v {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}
