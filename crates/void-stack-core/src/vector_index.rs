//! Vector index for semantic code search.
//!
//! Uses fastembed (BAAI/bge-small-en-v1.5) for embeddings and hnsw_rs for
//! approximate nearest neighbor search. Index persists to disk between sessions.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use hnsw_rs::api::AnnT;
use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::hnswio::HnswIo;
use hnsw_rs::prelude::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::ignore::VoidIgnore;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::security::is_sensitive_file;

/// Embedding dimension for BGE-small-en-v1.5 (used in tests).
#[cfg(test)]
const EMBED_DIM: usize = 384;
/// Lines per chunk (target).
const CHUNK_LINES: usize = 40;
/// Min lines for a chunk to be indexed.
const MIN_CHUNK_LINES: usize = 5;
/// Max file size to index (500KB).
const MAX_FILE_SIZE: u64 = 500_000;
/// HNSW parameters.
const HNSW_MAX_CONN: usize = 16;
const HNSW_MAX_LAYERS: usize = 16;
const HNSW_EF_CONSTRUCTION: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub file_path: String,
    pub chunk: String,
    pub score: f32,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub chunks_total: usize,
    pub model: String,
    pub size_mb: f64,
    pub created_at: DateTime<Utc>,
}

/// A code chunk with metadata.
struct Chunk {
    file_path: String,
    text: String,
    line_start: usize,
    line_end: usize,
}

// ── Paths ───────────────────────────────────────────────────

fn index_dir(project: &Project) -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("void-stack").join("indexes").join(&project.name)
}

fn meta_db_path(project: &Project) -> PathBuf {
    index_dir(project).join("meta.db")
}

fn hnsw_dir(project: &Project) -> PathBuf {
    index_dir(project).join("hnsw")
}

fn model_cache_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("void-stack").join("models")
}

// ── Public API ──────────────────────────────────────────────

/// Check if an index exists for a project.
pub fn index_exists(project: &Project) -> bool {
    meta_db_path(project).exists()
}

/// Get index statistics.
pub fn get_index_stats(project: &Project) -> Result<Option<IndexStats>, String> {
    if !index_exists(project) {
        return Ok(None);
    }
    let conn = open_meta_db(project)?;
    let stats = load_stats(&conn)?;
    Ok(Some(stats))
}

/// Delete the index for a project.
pub fn delete_index(project: &Project) -> Result<(), String> {
    let dir = index_dir(project);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to delete index: {}", e))?;
    }
    Ok(())
}

/// Index a project's codebase. `force` re-indexes everything.
/// `progress` callback receives (files_processed, total_files).
pub fn index_project(
    project: &Project,
    force: bool,
    progress: impl Fn(usize, usize),
) -> Result<IndexStats, String> {
    let project_path = PathBuf::from(strip_win_prefix(&project.path));

    // Collect files to index
    let files = collect_indexable_files(&project_path);
    let total = files.len();

    if total == 0 {
        return Err("No indexable files found in project".to_string());
    }

    // Setup dirs
    let idx_dir = index_dir(project);
    let _ = std::fs::create_dir_all(&idx_dir);
    let hnsw_path = hnsw_dir(project);
    let _ = std::fs::create_dir_all(&hnsw_path);

    // Open metadata DB
    let conn = open_meta_db(project)?;

    // Load existing file timestamps for incremental indexing
    let existing_timestamps = if !force {
        load_file_timestamps(&conn)?
    } else {
        // Clear everything for force re-index
        conn.execute_batch("DELETE FROM chunks;")
            .map_err(|e| e.to_string())?;
        std::collections::HashMap::new()
    };

    // Chunk files (only modified ones if incremental)
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut files_processed = 0usize;
    let mut _files_indexed = 0usize;
    let mut skipped_files: Vec<String> = Vec::new();

    for file_rel in &files {
        files_processed += 1;
        progress(files_processed, total);

        let abs_path = project_path.join(file_rel);
        let mtime = file_mtime(&abs_path);

        // Skip if not modified since last index
        if let Some(prev_mtime) = existing_timestamps.get(file_rel.as_str())
            && mtime <= *prev_mtime
            && !force
        {
            skipped_files.push(file_rel.clone());
            continue;
        }

        // Read file content
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Remove old chunks for this file
        let _ = conn.execute("DELETE FROM chunks WHERE file_path = ?1", [file_rel]);

        // Chunk the file
        let file_chunks = chunk_file(file_rel, &content);
        _files_indexed += 1;

        // Save chunks to DB
        for chunk in &file_chunks {
            conn.execute(
                "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![chunk.file_path, chunk.line_start, chunk.line_end, chunk.text, mtime],
            ).map_err(|e| e.to_string())?;
        }

        chunks.extend(file_chunks);
    }

    // Also re-add skipped file chunks from DB so we can rebuild the full index
    let mut all_chunks = chunks;
    if !force {
        let skipped_chunks = load_chunks_for_files(&conn, &skipped_files)?;
        all_chunks.extend(skipped_chunks);
    }

    let chunks_total = all_chunks.len();
    if chunks_total == 0 {
        return Err("No code chunks generated from project files".to_string());
    }

    // Generate embeddings
    let model = create_embedding_model()?;
    let texts: Vec<&str> = all_chunks.iter().map(|c| c.text.as_str()).collect();

    // Embed in batches
    let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
    for batch in texts.chunks(64) {
        let batch_vec: Vec<String> = batch.iter().map(|s| s.to_string()).collect();
        let embeddings = model
            .embed(batch_vec, None)
            .map_err(|e| format!("Embedding error: {}", e))?;
        all_embeddings.extend(embeddings);
    }

    // Build HNSW index
    let hnsw: Hnsw<f32, DistCosine> = Hnsw::new(
        HNSW_MAX_CONN,
        all_embeddings.len(),
        HNSW_MAX_LAYERS,
        HNSW_EF_CONSTRUCTION,
        DistCosine,
    );

    for (id, emb) in all_embeddings.iter().enumerate() {
        hnsw.insert((emb.as_slice(), id));
    }

    // Save HNSW to disk
    hnsw.file_dump(&hnsw_path, "index")
        .map_err(|e| format!("Failed to save HNSW index: {}", e))?;

    // Save chunk ID mapping (rowid in chunks table corresponds to HNSW id)
    // We need to store the order of chunks for lookup
    save_chunk_order(&conn, &all_chunks)?;

    // Save stats
    let size_mb = dir_size_mb(&idx_dir);
    let stats = IndexStats {
        files_indexed: files_processed,
        chunks_total,
        model: "BAAI/bge-small-en-v1.5".to_string(),
        size_mb,
        created_at: Utc::now(),
    };
    save_stats(&conn, &stats)?;

    // Record in token stats
    crate::stats::record_saving(crate::stats::TokenSavingsRecord {
        timestamp: Utc::now(),
        project: project.name.clone(),
        operation: "vector_index".to_string(),
        lines_original: chunks_total * CHUNK_LINES,
        lines_filtered: chunks_total,
        savings_pct: 0.0, // indexing itself doesn't save tokens
    });

    Ok(stats)
}

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

    let conn = open_meta_db(project)?;

    // Load chunk order
    let chunk_order = load_chunk_order(&conn)?;

    // Load HNSW
    let hnsw_path = hnsw_dir(project);

    let mut io = HnswIo::new(&hnsw_path, "index");
    let hnsw: Hnsw<f32, DistCosine> = io
        .load_hnsw()
        .map_err(|e| format!("Failed to load HNSW index: {}", e))?;

    // Embed query
    let model = create_embedding_model()?;
    let query_emb = model
        .embed(vec![query.to_string()], None)
        .map_err(|e| format!("Query embedding error: {}", e))?;

    if query_emb.is_empty() {
        return Err("Failed to generate query embedding".to_string());
    }

    let ef_search = top_k.max(HNSW_MAX_CONN);
    let neighbours = hnsw.search(&query_emb[0], top_k, ef_search);

    let mut results = Vec::with_capacity(neighbours.len());
    for neighbour in &neighbours {
        let hnsw_id = neighbour.d_id;
        if let Some(chunk_id) = chunk_order.get(hnsw_id)
            && let Ok(chunk) = load_chunk_by_id(&conn, *chunk_id)
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

fn create_embedding_model() -> Result<fastembed::TextEmbedding, String> {
    let cache_dir = model_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);

    let options = fastembed::InitOptions::new(fastembed::EmbeddingModel::BGESmallENV15)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(true);

    fastembed::TextEmbedding::try_new(options).map_err(|e| format!("Model init error: {}", e))
}

// ── File collection ─────────────────────────────────────────

/// Source code extensions worth indexing.
const CODE_EXTENSIONS: &[&str] = &[
    "rs",
    "go",
    "py",
    "js",
    "ts",
    "tsx",
    "jsx",
    "java",
    "kt",
    "swift",
    "dart",
    "c",
    "cpp",
    "h",
    "hpp",
    "cs",
    "rb",
    "php",
    "lua",
    "zig",
    "ex",
    "exs",
    "vue",
    "svelte",
    "astro",
    "toml",
    "yaml",
    "yml",
    "json",
    "proto",
    "sql",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "bat",
    "cmd",
    "md",
    "txt",
    "rst",
    "adoc",
    "dockerfile",
    "makefile",
    "justfile",
];

fn collect_indexable_files(project_path: &Path) -> Vec<String> {
    // Load ignore patterns
    let claudeignore = VoidIgnore::load_claudeignore(project_path);
    let voidignore = VoidIgnore::load(project_path);

    let mut files = Vec::new();
    collect_files_recursive(
        project_path,
        project_path,
        &mut files,
        &claudeignore,
        &voidignore,
        6, // deeper than file_reader's 3 levels
    );
    files
}

fn collect_files_recursive(
    root: &Path,
    current: &Path,
    files: &mut Vec<String>,
    claudeignore: &VoidIgnore,
    voidignore: &VoidIgnore,
    depth: u32,
) {
    if depth == 0 {
        return;
    }
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden, build dirs, etc.
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "__pycache__"
            || name == "dist"
            || name == "build"
            || name == ".git"
            || name == "vendor"
            || name == ".venv"
        {
            continue;
        }

        if let Ok(rel) = path.strip_prefix(root) {
            let rel_str = rel.to_string_lossy().to_string();
            if claudeignore.is_ignored(&rel_str) || voidignore.is_ignored(&rel_str) {
                continue;
            }
        }

        if path.is_dir() {
            collect_files_recursive(root, &path, files, claudeignore, voidignore, depth - 1);
        } else if path.is_file() {
            // Check extension
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let file_name_lower = name.to_lowercase();

            let is_code = CODE_EXTENSIONS.contains(&ext.as_str())
                || CODE_EXTENSIONS.contains(&file_name_lower.as_str());

            if !is_code {
                continue;
            }

            // Skip sensitive files
            if is_sensitive_file(&path) {
                continue;
            }

            // Skip large files
            if let Ok(meta) = path.metadata()
                && meta.len() > MAX_FILE_SIZE
            {
                continue;
            }

            if let Ok(rel) = path.strip_prefix(root) {
                files.push(rel.to_string_lossy().to_string());
            }
        }
    }
}

// ── Chunking ────────────────────────────────────────────────

fn chunk_file(file_path: &str, content: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() < MIN_CHUNK_LINES {
        if lines.is_empty() {
            return vec![];
        }
        return vec![Chunk {
            file_path: file_path.to_string(),
            text: format!("// {}\n{}", file_path, content),
            line_start: 1,
            line_end: lines.len(),
        }];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < lines.len() {
        let mut end = (start + CHUNK_LINES).min(lines.len());

        // Try to break at a blank line near the target
        if end < lines.len() {
            let search_start = (start + CHUNK_LINES - 10).max(start);
            let search_end = (start + CHUNK_LINES + 10).min(lines.len());
            for i in (search_start..search_end).rev() {
                if lines[i].trim().is_empty() {
                    end = i + 1;
                    break;
                }
            }
        }

        // Don't create tiny trailing chunks
        if lines.len() - end < MIN_CHUNK_LINES {
            end = lines.len();
        }

        let chunk_text = lines[start..end].join("\n");
        chunks.push(Chunk {
            file_path: file_path.to_string(),
            text: format!("// {}\n{}", file_path, chunk_text),
            line_start: start + 1,
            line_end: end,
        });

        start = end;
    }

    chunks
}

// ── Metadata DB ─────────────────────────────────────────────

fn open_meta_db(project: &Project) -> Result<Connection, String> {
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
    Ok(conn)
}

fn load_file_timestamps(
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

fn load_chunks_for_files(conn: &Connection, files: &[String]) -> Result<Vec<Chunk>, String> {
    let mut chunks = Vec::new();
    for file in files {
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line_start, line_end, text FROM chunks WHERE file_path = ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([file], |row| {
                Ok(Chunk {
                    file_path: row.get(0)?,
                    text: row.get(3)?,
                    line_start: row.get::<_, i64>(1)? as usize,
                    line_end: row.get::<_, i64>(2)? as usize,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            chunks.push(row);
        }
    }
    Ok(chunks)
}

fn save_chunk_order(conn: &Connection, chunks: &[Chunk]) -> Result<(), String> {
    conn.execute("DELETE FROM chunk_order", [])
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT id FROM chunks WHERE file_path = ?1 AND line_start = ?2 AND line_end = ?3 LIMIT 1")
        .map_err(|e| e.to_string())?;

    let mut insert_stmt = conn
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
    Ok(())
}

fn load_chunk_order(conn: &Connection) -> Result<Vec<i64>, String> {
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

fn load_chunk_by_id(conn: &Connection, chunk_id: i64) -> Result<Chunk, String> {
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

fn save_stats(conn: &Connection, stats: &IndexStats) -> Result<(), String> {
    let json = serde_json::to_string(stats).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO stats (key, value) VALUES ('index_stats', ?1)",
        [&json],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn load_stats(conn: &Connection) -> Result<IndexStats, String> {
    let json: String = conn
        .query_row(
            "SELECT value FROM stats WHERE key = 'index_stats'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

// ── Helpers ─────────────────────────────────────────────────

fn file_mtime(path: &Path) -> f64 {
    path.metadata()
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        })
        .unwrap_or(0.0)
}

fn dir_size_mb(dir: &Path) -> f64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
            if entry.path().is_dir() {
                total += (dir_size_mb(&entry.path()) * 1_048_576.0) as u64;
            }
        }
    }
    total as f64 / 1_048_576.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_small_file() {
        let chunks = chunk_file("test.rs", "fn main() {\n    println!(\"hello\");\n}");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("test.rs"));
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks[0].line_end, 3);
    }

    #[test]
    fn test_chunk_empty_file() {
        let chunks = chunk_file("empty.rs", "");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_large_file() {
        let lines: Vec<String> = (0..120).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");
        let chunks = chunk_file("big.rs", &content);
        assert!(chunks.len() >= 2);

        // All lines should be covered
        let total_lines: usize = chunks.iter().map(|c| c.line_end - c.line_start + 1).sum();
        assert!(total_lines >= 120);

        // Each chunk should have the file path prefix
        for chunk in &chunks {
            assert!(chunk.text.contains("big.rs"));
        }
    }

    #[test]
    fn test_chunk_respects_blank_lines() {
        let mut lines = Vec::new();
        for i in 0..50 {
            lines.push(format!("code line {}", i));
        }
        lines[35] = String::new(); // blank line near chunk boundary
        let content = lines.join("\n");
        let chunks = chunk_file("test.go", &content);
        // Should have at least 1 chunk
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunk_min_lines_threshold() {
        let content = "a\nb\nc";
        let chunks = chunk_file("tiny.rs", content);
        // 3 lines is below MIN_CHUNK_LINES, should still produce 1 chunk
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_meta_db_creation() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: "test-project".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        // We can't easily test the full path without mocking dirs, but we can test the DB init
        let db_path = dir.path().join("test_meta.db");
        let conn = Connection::open(&db_path).unwrap();
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
            );",
        )
        .unwrap();

        // Insert and read back
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["src/main.rs", 1i64, 10i64, "fn main() {}", 1000.0],
        ).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_index_stats_serialization() {
        let stats = IndexStats {
            files_indexed: 42,
            chunks_total: 300,
            model: "BAAI/bge-small-en-v1.5".to_string(),
            size_mb: 15.5,
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("42"));
        assert!(json.contains("bge-small"));
        let parsed: IndexStats = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.files_indexed, 42);
    }

    #[test]
    fn test_search_result_serialization() {
        let result = SearchResult {
            file_path: "src/main.rs".to_string(),
            chunk: "fn main() {}".to_string(),
            score: 0.95,
            line_start: 1,
            line_end: 10,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("main.rs"));
        assert!(json.contains("0.95"));
    }

    #[test]
    fn test_code_extensions_coverage() {
        assert!(CODE_EXTENSIONS.contains(&"rs"));
        assert!(CODE_EXTENSIONS.contains(&"go"));
        assert!(CODE_EXTENSIONS.contains(&"py"));
        assert!(CODE_EXTENSIONS.contains(&"ts"));
        assert!(CODE_EXTENSIONS.contains(&"dart"));
        assert!(CODE_EXTENSIONS.contains(&"proto"));
    }
}
