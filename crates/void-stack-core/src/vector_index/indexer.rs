//! Project indexing: background jobs, file collection, and HNSW building.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::prelude::*;
use serde::Serialize;

use super::chunker::{CHUNK_LINES, Chunk, chunk_file};
use super::db;
use super::search::{
    HNSW_EF_CONSTRUCTION, HNSW_MAX_CONN, HNSW_MAX_LAYERS, embed_texts, invalidate_hnsw_cache,
};
use super::stats::{IndexStats, dir_size_mb, file_mtime, hnsw_dir, index_dir, save_stats};
use crate::ignore::VoidIgnore;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::security::is_sensitive_file;

// ── Job registry ───────────────────────────────────────────

/// Global job registry for background indexing: project_path → status
static INDEX_JOBS: OnceLock<Mutex<HashMap<String, IndexJobStatus>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
pub enum IndexJobStatus {
    Running {
        files_processed: usize,
        files_total: usize,
    },
    Completed {
        stats: IndexStats,
    },
    Failed {
        error: String,
    },
}

pub(crate) fn job_registry() -> &'static Mutex<HashMap<String, IndexJobStatus>> {
    INDEX_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Update job status, recovering from poison if needed.
pub(crate) fn update_job(key: &str, status: IndexJobStatus) {
    let registry = job_registry();
    match registry.lock() {
        Ok(mut jobs) => {
            jobs.insert(key.to_string(), status);
        }
        Err(poisoned) => {
            poisoned.into_inner().insert(key.to_string(), status);
        }
    }
}

/// Read job status, recovering from poison if needed.
pub(crate) fn read_job(key: &str) -> Option<IndexJobStatus> {
    let registry = job_registry();
    match registry.lock() {
        Ok(jobs) => jobs.get(key).cloned(),
        Err(poisoned) => poisoned.into_inner().get(key).cloned(),
    }
}

// ── Public API ──────────────────────────────────────────────

/// Start indexing in a background thread. Returns immediately with job key.
pub fn index_project_background(project: &Project, force: bool) -> String {
    let job_key = project.path.clone();

    // Mark as running immediately (poison-safe)
    update_job(
        &job_key,
        IndexJobStatus::Running {
            files_processed: 0,
            files_total: 0,
        },
    );

    let project_clone = project.clone();
    let key = job_key.clone();

    std::thread::spawn(move || {
        let result = index_project(&project_clone, force, |processed, total| {
            update_job(
                &key,
                IndexJobStatus::Running {
                    files_processed: processed,
                    files_total: total,
                },
            );
        });

        match result {
            Ok(stats) => {
                update_job(&key, IndexJobStatus::Completed { stats });
            }
            Err(e) => {
                update_job(&key, IndexJobStatus::Failed { error: e });
            }
        }
    });

    job_key
}

/// Get the current status of an indexing job (poison-safe).
pub fn get_index_job_status(project: &Project) -> Option<IndexJobStatus> {
    read_job(&project.path)
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
    let conn = db::open_meta_db(project)?;

    // Load existing file timestamps for incremental indexing
    let existing_timestamps = if !force {
        db::load_file_timestamps(&conn)?
    } else {
        // Clear everything for force re-index
        conn.execute_batch("DELETE FROM chunks;")
            .map_err(|e| e.to_string())?;
        std::collections::HashMap::new()
    };

    // Chunk files (only modified ones if incremental)
    let mut new_chunks: Vec<Chunk> = Vec::new();
    let mut files_processed = 0usize;
    let mut skipped_files: Vec<String> = Vec::new();

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

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
        let _ = tx.execute("DELETE FROM chunks WHERE file_path = ?1", [file_rel]);

        // Chunk the file
        let file_chunks = chunk_file(file_rel, &content);

        // Save chunks to DB (embedding will be added after embedding)
        for chunk in &file_chunks {
            tx.execute(
                "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![chunk.file_path, chunk.line_start, chunk.line_end, chunk.text, mtime],
            ).map_err(|e| e.to_string())?;
        }

        new_chunks.extend(file_chunks);
    }

    tx.commit().map_err(|e| e.to_string())?;

    // Load cached embeddings for unchanged files from SQLite
    let mut all_chunks: Vec<Chunk> = Vec::new();
    let mut all_embeddings: Vec<Vec<f32>> = Vec::new();

    if !force && !skipped_files.is_empty() {
        let cached = db::load_chunks_with_embeddings(&conn, &skipped_files)?;
        for (chunk, embedding) in cached {
            all_chunks.push(chunk);
            all_embeddings.push(embedding);
        }
    }

    // Generate embeddings ONLY for new/modified chunks
    if !new_chunks.is_empty() {
        let new_texts: Vec<String> = new_chunks.iter().map(|c| c.text.clone()).collect();
        let mut new_embeddings: Vec<Vec<f32>> = Vec::with_capacity(new_texts.len());
        for batch in new_texts.chunks(64) {
            let embeddings = embed_texts(&batch.to_vec())?;
            new_embeddings.extend(embeddings);
        }

        // Save new embeddings to SQLite
        db::save_embeddings(&conn, &new_chunks, &new_embeddings)?;

        all_chunks.extend(new_chunks);
        all_embeddings.extend(new_embeddings);
    }

    let chunks_total = all_chunks.len();
    if chunks_total == 0 {
        return Err("No code chunks generated from project files".to_string());
    }

    // Build HNSW index from all embeddings (cached + new)
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

    // Write HNSW to a temp dir, then rename atomically to avoid
    // concurrent readers seeing a half-written index
    let tmp_path = hnsw_path.with_extension("_building");
    let _ = std::fs::create_dir_all(&tmp_path);
    hnsw.file_dump(&tmp_path, "index")
        .map_err(|e| format!("Failed to save HNSW index: {}", e))?;
    // Remove old hnsw dir and rename tmp into place
    if hnsw_path.exists() {
        let _ = std::fs::remove_dir_all(&hnsw_path);
    }
    std::fs::rename(&tmp_path, &hnsw_path)
        .map_err(|e| format!("Failed to finalize HNSW index: {}", e))?;
    invalidate_hnsw_cache(project);

    // Save chunk ID mapping
    db::save_chunk_order(&conn, &all_chunks)?;

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

// ── File collection ─────────────────────────────────────────

/// Source code extensions worth indexing.
pub(crate) const CODE_EXTENSIONS: &[&str] = &[
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

/// Max file size to index (500KB).
const MAX_FILE_SIZE: u64 = 500_000;

pub(crate) fn collect_indexable_files(project_path: &Path) -> Vec<String> {
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

        // Skip hidden dirs, build artifacts, deps, generated code
        if name.starts_with('.')
            || matches!(
                name.as_str(),
                "node_modules"
                    | "target"
                    | "__pycache__"
                    | "dist"
                    | "build"
                    | ".git"
                    | "vendor"
                    | ".venv"
                    | "venv"
                    | ".env"
                    | ".next"
                    | ".nuxt"
                    | ".dart_tool"
                    | ".turbo"
                    | "coverage"
            )
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

            // Skip generated/auto-generated files
            if name.ends_with(".pb.rs")
                || name.ends_with("_pb2.py")
                || name.ends_with(".pb.go")
                || name.ends_with(".g.dart")
                || name.ends_with(".freezed.dart")
                || name.ends_with(".gen.go")
                || name == "lcov.info"
                || name == "coverage.xml"
            {
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
