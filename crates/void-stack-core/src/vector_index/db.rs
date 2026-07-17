//! SQLite metadata database: chunks, embeddings, chunk order.

use rusqlite::Connection;

use crate::error::IndexError;

use super::chunker::Chunk;
use super::stats::meta_db_path;
use crate::model::Project;

pub(crate) fn open_meta_db(project: &Project) -> Result<Connection, IndexError> {
    let path = meta_db_path(project);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(&path).map_err(IndexError::from)?;
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
    .map_err(IndexError::from)?;

    // Migration: add embedding column if not present
    let has_embedding: bool = conn
        .prepare("PRAGMA table_info(chunks)")
        .map(|mut stmt| {
            stmt.query_map([], |row| row.get::<_, String>(1))
                .map(|rows| rows.flatten().any(|name| name == "embedding"))
                .unwrap_or(false)
        })
        .unwrap_or(false);

    if !has_embedding {
        conn.execute_batch("ALTER TABLE chunks ADD COLUMN embedding BLOB")
            .map_err(IndexError::from)?;
    }

    // Migration: add file_hash column for SHA-256 content-based incremental indexing.
    // Ignore errors — the column may already exist from a prior run.
    let _ = conn.execute_batch("ALTER TABLE chunks ADD COLUMN file_hash TEXT NOT NULL DEFAULT '';");
    let _ = conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_chunks_file_hash ON chunks(file_path, file_hash);",
    );

    // Lexical (BM25) index over the same chunks. `tokenchars '_'` keeps
    // snake_case identifiers as single tokens so exact-identifier queries
    // hit. rowid mirrors chunks.id; the indexing pipeline keeps both in
    // sync (same per-file delete/insert, same SHA-256 invalidation).
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            text, file_path, tokenize = \"unicode61 tokenchars '_'\");",
    )
    .map_err(IndexError::from)?;

    // Lazy backfill for indexes built before the FTS table existed.
    let fts_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
        .unwrap_or(0);
    let chunk_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap_or(0);
    if fts_rows == 0 && chunk_rows > 0 {
        conn.execute_batch(
            "INSERT INTO chunks_fts(rowid, text, file_path)
             SELECT id, text, file_path FROM chunks;",
        )
        .map_err(IndexError::from)?;
    }

    Ok(conn)
}

/// Mirror a per-file chunk replacement into the FTS index.
pub(crate) fn fts_replace_file(conn: &Connection, file_path: &str) -> Result<(), IndexError> {
    conn.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_path = ?1)",
        [file_path],
    )
    .map_err(IndexError::from)?;
    // Also drop FTS rows whose chunk row no longer exists at all.
    conn.execute(
        "DELETE FROM chunks_fts WHERE rowid NOT IN (SELECT id FROM chunks)",
        [],
    )
    .map_err(IndexError::from)?;
    conn.execute(
        "INSERT INTO chunks_fts(rowid, text, file_path)
         SELECT id, text, file_path FROM chunks WHERE file_path = ?1",
        [file_path],
    )
    .map_err(IndexError::from)?;
    Ok(())
}

pub(crate) fn load_file_timestamps(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, f64>, IndexError> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT file_path, MAX(mtime) FROM chunks GROUP BY file_path")
        .map_err(IndexError::from)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })
        .map_err(IndexError::from)?;
    let mut map = std::collections::HashMap::new();
    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }
    Ok(map)
}

/// Load the cached SHA-256 hash (lowercase hex) for every indexed file.
/// Files without a cached hash are omitted.
pub(crate) fn load_file_hashes(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, String>, IndexError> {
    let mut stmt = conn
        .prepare(
            "SELECT file_path, file_hash FROM chunks \
             WHERE file_hash != '' GROUP BY file_path",
        )
        .map_err(IndexError::from)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(IndexError::from)?;
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
) -> Result<Vec<(Chunk, Vec<f32>)>, IndexError> {
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
        let mut stmt = conn.prepare(&sql).map_err(IndexError::from)?;

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
            .map_err(IndexError::from)?;

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
) -> Result<(), IndexError> {
    let tx = conn.unchecked_transaction().map_err(IndexError::from)?;
    let mut stmt = tx
        .prepare(
            "UPDATE chunks SET embedding = ?1 WHERE file_path = ?2 AND line_start = ?3 AND line_end = ?4",
        )
        .map_err(IndexError::from)?;

    let mut failed = 0usize;
    for (chunk, emb) in chunks.iter().zip(embeddings.iter()) {
        let blob = f32_vec_to_bytes(emb);
        if let Err(e) = stmt.execute(rusqlite::params![
            blob,
            chunk.file_path,
            chunk.line_start as i64,
            chunk.line_end as i64,
        ]) {
            tracing::warn!(
                file = %chunk.file_path,
                lines = %format!("{}-{}", chunk.line_start, chunk.line_end),
                error = %e,
                "Failed to save embedding"
            );
            failed += 1;
        }
    }
    if failed > 0 {
        tracing::warn!("{}/{} embeddings failed to save", failed, chunks.len());
    }

    drop(stmt);
    tx.commit().map_err(IndexError::from)?;
    Ok(())
}

pub(crate) fn save_chunk_order(conn: &Connection, chunks: &[Chunk]) -> Result<(), IndexError> {
    let tx = conn.unchecked_transaction().map_err(IndexError::from)?;

    tx.execute("DELETE FROM chunk_order", [])
        .map_err(IndexError::from)?;

    let mut stmt = tx
        .prepare("SELECT id FROM chunks WHERE file_path = ?1 AND line_start = ?2 AND line_end = ?3 LIMIT 1")
        .map_err(IndexError::from)?;

    let mut insert_stmt = tx
        .prepare("INSERT INTO chunk_order (hnsw_id, chunk_id) VALUES (?1, ?2)")
        .map_err(IndexError::from)?;

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
    tx.commit().map_err(IndexError::from)?;
    Ok(())
}

pub(crate) fn load_chunk_order(conn: &Connection) -> Result<Vec<i64>, IndexError> {
    let count: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(hnsw_id), -1) FROM chunk_order",
            [],
            |r| r.get(0),
        )
        .map_err(IndexError::from)?;

    if count < 0 {
        return Ok(vec![]);
    }

    let mut order = vec![0i64; (count + 1) as usize];
    let mut stmt = conn
        .prepare("SELECT hnsw_id, chunk_id FROM chunk_order")
        .map_err(IndexError::from)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)? as usize, row.get::<_, i64>(1)?))
        })
        .map_err(IndexError::from)?;
    for row in rows.flatten() {
        if row.0 < order.len() {
            order[row.0] = row.1;
        }
    }
    Ok(order)
}

pub(crate) fn load_chunk_by_id(conn: &Connection, chunk_id: i64) -> Result<Chunk, IndexError> {
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
    .map_err(IndexError::from)
}

// ── Serialization helpers ──────────────────────────────────

fn f32_vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &val in v {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Decode a little-endian f32 blob (as written by [`f32_vec_to_bytes`]).
/// Shared with [`crate::vector_index::cluster`] so both sides decode the
/// same way.
pub(crate) fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture project pointing at a tempdir. `name` must be unique per test
    /// because all tests share the isolated per-process data dir.
    fn fixture_project(name: &str, dir: &tempfile::TempDir) -> Project {
        Project {
            name: format!("db-{}-fixture-{}", name, std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    fn insert_chunk(conn: &Connection, file: &str, ls: i64, le: i64, text: &str) {
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
             VALUES (?1, ?2, ?3, ?4, 42.0)",
            rusqlite::params![file, ls, le, text],
        )
        .unwrap();
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    // ── Schema & migrations ─────────────────────────────────

    #[test]
    fn test_open_meta_db_creates_schema() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("schema", &dir);

        let conn = open_meta_db(&project).expect("open must succeed");

        for table in ["chunks", "chunk_order", "stats", "chunks_fts"] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table '{}' must exist", table);
        }

        // Migration columns must be present on a fresh database too.
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(chunks)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .flatten()
            .collect();
        assert!(cols.contains(&"embedding".to_string()), "embedding column");
        assert!(cols.contains(&"file_hash".to_string()), "file_hash column");

        // Re-opening must be idempotent (migrations tolerate existing state).
        drop(conn);
        open_meta_db(&project).expect("second open must succeed");
    }

    #[test]
    fn test_open_meta_db_backfills_fts_from_existing_chunks() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("backfill", &dir);

        let conn = open_meta_db(&project).unwrap();
        insert_chunk(&conn, "a.rs", 1, 10, "fn alpha() {}");
        insert_chunk(&conn, "b.rs", 1, 10, "fn beta() {}");
        // Simulate an index built before the FTS table existed.
        conn.execute("DELETE FROM chunks_fts", []).unwrap();
        drop(conn);

        let conn = open_meta_db(&project).unwrap();
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM chunks_fts"),
            2,
            "reopen must lazily backfill FTS rows from chunks"
        );
    }

    // ── FTS mirroring ───────────────────────────────────────

    #[test]
    fn test_fts_replace_file_keeps_index_in_sync() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("fts", &dir);
        let conn = open_meta_db(&project).unwrap();

        insert_chunk(&conn, "a.rs", 1, 10, "fn old_name() {}");
        insert_chunk(&conn, "other.rs", 1, 5, "fn untouched() {}");
        fts_replace_file(&conn, "a.rs").unwrap();
        fts_replace_file(&conn, "other.rs").unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM chunks_fts"), 2);

        // Re-index a.rs: replace its chunk rows, then mirror into FTS.
        conn.execute("DELETE FROM chunks WHERE file_path = 'a.rs'", [])
            .unwrap();
        insert_chunk(&conn, "a.rs", 1, 12, "fn new_name() {}");
        fts_replace_file(&conn, "a.rs").unwrap();

        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM chunks_fts"),
            2,
            "stale FTS rows must be dropped, one row per live chunk"
        );
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'new_name'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1, "new text must be searchable");
        let stale: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'old_name'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale, 0, "old text must no longer be searchable");
    }

    // ── Timestamps & hashes ─────────────────────────────────

    #[test]
    fn test_load_file_timestamps_returns_max_mtime_per_file() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("mtimes", &dir);
        let conn = open_meta_db(&project).unwrap();

        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
             VALUES ('a.rs', 1, 10, 'x', 100.0), ('a.rs', 11, 20, 'y', 200.0), \
                    ('b.rs', 1, 5, 'z', 50.0)",
            [],
        )
        .unwrap();

        let map = load_file_timestamps(&conn).unwrap();
        assert_eq!(map.len(), 2, "one entry per distinct file");
        assert_eq!(map["a.rs"], 200.0, "max mtime wins for a.rs");
        assert_eq!(map["b.rs"], 50.0);
    }

    #[test]
    fn test_load_file_hashes_omits_empty_hashes() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("hashes", &dir);
        let conn = open_meta_db(&project).unwrap();

        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime, file_hash) \
             VALUES ('hashed.rs', 1, 10, 'x', 0, 'abc123'), \
                    ('legacy.rs', 1, 10, 'y', 0, '')",
            [],
        )
        .unwrap();

        let map = load_file_hashes(&conn).unwrap();
        assert_eq!(
            map.get("hashed.rs").map(String::as_str),
            Some("abc123"),
            "cached hash must be returned"
        );
        assert!(
            !map.contains_key("legacy.rs"),
            "files without a cached hash must be omitted"
        );
    }

    // ── Embeddings ──────────────────────────────────────────

    #[test]
    fn test_save_and_load_embeddings_roundtrip() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("embed", &dir);
        let conn = open_meta_db(&project).unwrap();

        insert_chunk(&conn, "a.rs", 1, 10, "fn alpha() {}");
        insert_chunk(&conn, "a.rs", 11, 20, "fn alpha_two() {}");

        let chunk = Chunk {
            file_path: "a.rs".to_string(),
            text: "fn alpha() {}".to_string(),
            line_start: 1,
            line_end: 10,
        };
        let embedding = vec![0.25f32, -1.5, 3.0];
        save_embeddings(
            &conn,
            std::slice::from_ref(&chunk),
            std::slice::from_ref(&embedding),
        )
        .unwrap();

        let loaded = load_chunks_with_embeddings(&conn, &["a.rs".to_string()]).unwrap();
        assert_eq!(
            loaded.len(),
            1,
            "only the chunk with an embedding is returned; the other is skipped"
        );
        let (c, e) = &loaded[0];
        assert_eq!(c.file_path, "a.rs");
        assert_eq!(c.line_start, 1);
        assert_eq!(c.line_end, 10);
        assert_eq!(*e, embedding, "embedding must round-trip bit-exact");

        // Empty file list short-circuits.
        assert!(
            load_chunks_with_embeddings(&conn, &[]).unwrap().is_empty(),
            "no files requested means no results"
        );
        // Unknown file yields nothing.
        assert!(
            load_chunks_with_embeddings(&conn, &["ghost.rs".to_string()])
                .unwrap()
                .is_empty()
        );
    }

    // ── Chunk order ─────────────────────────────────────────

    #[test]
    fn test_chunk_order_roundtrip_and_missing_chunks_skipped() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("order", &dir);
        let conn = open_meta_db(&project).unwrap();

        assert!(
            load_chunk_order(&conn).unwrap().is_empty(),
            "empty table yields empty order"
        );

        insert_chunk(&conn, "a.rs", 1, 10, "one");
        insert_chunk(&conn, "b.rs", 1, 10, "two");
        let mk = |file: &str| Chunk {
            file_path: file.to_string(),
            text: String::new(),
            line_start: 1,
            line_end: 10,
        };
        // hnsw order: b first, then a, then a phantom not present in the DB.
        let chunks = vec![mk("b.rs"), mk("a.rs"), mk("ghost.rs")];
        save_chunk_order(&conn, &chunks).unwrap();

        let order = load_chunk_order(&conn).unwrap();
        assert_eq!(order.len(), 2, "phantom chunk contributes no mapping");
        let id_a: i64 = conn
            .query_row("SELECT id FROM chunks WHERE file_path = 'a.rs'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let id_b: i64 = conn
            .query_row("SELECT id FROM chunks WHERE file_path = 'b.rs'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(order[0], id_b, "hnsw_id 0 maps to b.rs");
        assert_eq!(order[1], id_a, "hnsw_id 1 maps to a.rs");

        // Saving again replaces the previous order wholesale.
        save_chunk_order(&conn, &[mk("a.rs")]).unwrap();
        let order = load_chunk_order(&conn).unwrap();
        assert_eq!(order, vec![id_a], "old order must be cleared on re-save");
    }

    #[test]
    fn test_load_chunk_by_id() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("byid", &dir);
        let conn = open_meta_db(&project).unwrap();

        insert_chunk(&conn, "a.rs", 3, 9, "fn body() {}");
        let id: i64 = conn
            .query_row("SELECT id FROM chunks", [], |r| r.get(0))
            .unwrap();

        let chunk = load_chunk_by_id(&conn, id).expect("existing id loads");
        assert_eq!(chunk.file_path, "a.rs");
        assert_eq!(chunk.line_start, 3);
        assert_eq!(chunk.line_end, 9);
        assert_eq!(chunk.text, "fn body() {}");

        assert!(
            load_chunk_by_id(&conn, id + 999).is_err(),
            "missing id must be an error"
        );
    }

    // ── Serialization helpers ───────────────────────────────

    #[test]
    fn test_f32_bytes_roundtrip() {
        let values = vec![0.0f32, 1.5, -2.25, f32::MAX, f32::MIN_POSITIVE];
        let bytes = f32_vec_to_bytes(&values);
        assert_eq!(bytes.len(), values.len() * 4, "4 bytes per f32");
        assert_eq!(
            bytes_to_f32_vec(&bytes),
            values,
            "encode/decode must be lossless"
        );

        assert!(bytes_to_f32_vec(&[]).is_empty(), "empty blob decodes empty");
        // Trailing bytes that don't form a full f32 are ignored.
        let ragged = [0u8, 0, 192, 63, 1]; // 1.5f32 LE + one stray byte
        assert_eq!(
            bytes_to_f32_vec(&ragged),
            vec![1.5f32],
            "partial trailing word must be dropped"
        );
    }
}
