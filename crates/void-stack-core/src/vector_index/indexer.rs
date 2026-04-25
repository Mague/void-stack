//! Project indexing: background jobs, file collection, and HNSW building.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::prelude::*;
use serde::Serialize;

use super::chunker::{CHUNK_LINES, Chunk, chunk_file, enrich_chunk_with_context};
use super::db;
use super::search::{
    HNSW_EF_CONSTRUCTION, HNSW_MAX_CONN, HNSW_MAX_LAYERS, embed_texts, invalidate_hnsw_cache,
};
use super::stats::{
    IndexStats, dir_size_mb, file_mtime, file_sha256, get_git_changed_files, hnsw_dir, index_dir,
    save_stats,
};
use crate::ignore::VoidIgnore;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::security::is_sensitive_file;

// ── File watching ───────────────────────────────────────────

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Debounce window for coalescing rapid file events (autoformat, save-all).
const WATCH_DEBOUNCE_MS: u64 = 500;

/// Global watch registry: project_path → active watcher.
static WATCH_REGISTRY: OnceLock<Mutex<HashMap<String, RecommendedWatcher>>> = OnceLock::new();

fn watch_registry() -> &'static Mutex<HashMap<String, RecommendedWatcher>> {
    WATCH_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start watching a project directory. File changes trigger an incremental
/// re-index ~500 ms after the last event (debounced to absorb bursts from
/// autoformat or save-all). Call `unwatch_project` to stop.
pub fn watch_project(project: &Project) -> Result<(), String> {
    let project_path = PathBuf::from(strip_win_prefix(&project.path));
    let project_clone = project.clone();
    let key = project.path.clone();

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
    let mut watcher =
        notify::recommended_watcher(tx).map_err(|e| format!("Failed to create watcher: {}", e))?;

    watcher
        .watch(&project_path, RecursiveMode::Recursive)
        .map_err(|e| format!("Failed to watch path: {}", e))?;

    let watch_path = project_path.clone();
    let ignore = VoidIgnore::load(&watch_path);

    std::thread::spawn(move || {
        let mut last_event = std::time::Instant::now();
        let mut pending = false;

        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Ok(event)) => {
                    let is_relevant = matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                    ) && event.paths.iter().any(|p| {
                        let rel = p.strip_prefix(&watch_path).unwrap_or(p);
                        let rel_str = rel.to_string_lossy();
                        !ignore.is_ignored(&rel_str)
                            && !rel_str.contains(".void-stack")
                            && !rel_str.contains("target/")
                            && !rel_str.contains("node_modules/")
                            && !rel_str.contains(".git/")
                    });

                    if is_relevant {
                        last_event = std::time::Instant::now();
                        pending = true;
                    }
                }
                Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }

            if pending
                && last_event.elapsed() >= std::time::Duration::from_millis(WATCH_DEBOUNCE_MS)
            {
                pending = false;
                // Filesystem watch reacts to save events, not commits. We
                // want the indexer to use SHA-256 to figure out which files
                // actually changed (which is what passing `None` does);
                // `Some("HEAD")` would diff HEAD against the working tree
                // and silently re-index nothing whenever everything is
                // already committed.
                index_project_background(&project_clone, false, None);
            }
        }
    });

    let mut registry = watch_registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.insert(key, watcher);

    Ok(())
}

/// Stop watching a project (idempotent).
pub fn unwatch_project(project: &Project) {
    let mut registry = watch_registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.remove(&project.path);
}

/// Whether `watch_project` is currently active for this project.
pub fn is_watching(project: &Project) -> bool {
    let registry = watch_registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.contains_key(&project.path)
}

/// Install a git `post-commit` hook that triggers an incremental re-index
/// after each commit. Appends to an existing hook if present and is a no-op
/// when the hook already contains a `void index` line. Returns an error if
/// the directory is not a git repo.
pub fn install_git_hook(project: &Project) -> Result<(), String> {
    let project_path = PathBuf::from(strip_win_prefix(&project.path));
    let hooks_dir = project_path.join(".git").join("hooks");

    if !hooks_dir.exists() {
        return Err("Not a git repository".to_string());
    }

    let hook_path = hooks_dir.join("post-commit");
    // HEAD~1 compares the freshly-made commit against its parent, which is
    // what we want post-commit. Using plain HEAD would diff HEAD against
    // the working tree — empty right after a commit, so nothing re-indexes.
    let hook_line = format!(
        "#!/bin/sh\n# Auto-generated by void-stack\nvoid index {} --git-base HEAD~1 2>/dev/null &\n",
        project.name
    );

    let existing = std::fs::read_to_string(&hook_path).unwrap_or_default();
    if existing.contains("void index") {
        return Ok(());
    }

    let new_content = if existing.is_empty() {
        hook_line
    } else {
        format!("{}\n{}", existing.trim_end(), hook_line)
    };

    std::fs::write(&hook_path, new_content).map_err(|e| format!("Failed to write hook: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms).map_err(|e| e.to_string())?;
    }

    Ok(())
}

// ── Stale-file cleanup ──────────────────────────────────────

/// Drop chunks for files present in the DB but no longer in the indexable
/// set. Mutates `existing_timestamps` / `existing_hashes` in place so the
/// main loop doesn't try to re-use stale entries. Returns how many files
/// were removed.
pub(crate) fn cleanup_stale_chunks(
    conn: &rusqlite::Connection,
    current_files: &[String],
    existing_timestamps: &mut HashMap<String, f64>,
    existing_hashes: &mut HashMap<String, String>,
) -> Result<usize, String> {
    if existing_timestamps.is_empty() {
        return Ok(0);
    }
    let current: std::collections::HashSet<&str> =
        current_files.iter().map(|s| s.as_str()).collect();
    let stale: Vec<String> = existing_timestamps
        .keys()
        .filter(|f| !current.contains(f.as_str()))
        .cloned()
        .collect();
    if stale.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for file in &stale {
        tx.execute("DELETE FROM chunks WHERE file_path = ?1", [file])
            .map_err(|e| e.to_string())?;
        existing_timestamps.remove(file);
        existing_hashes.remove(file);
    }
    // Drop `chunk_order` rows whose `chunk_id` no longer exists in `chunks`.
    // Without this, the HNSW layer can return a hnsw_id that resolves to a
    // missing row and surfaces phantom results in semantic_search.
    tx.execute(
        "DELETE FROM chunk_order WHERE chunk_id NOT IN (SELECT id FROM chunks)",
        [],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(stale.len())
}

// ── Import context for embeddings ───────────────────────────

/// Build a lightweight import map for the project used to enrich chunk
/// text before embedding. Maps file → (imports, imported_by) where both
/// sides are relative paths within the project. External edges are skipped.
/// Returns an empty map when `build_graph` can't determine a language.
fn build_import_context(
    project_path: &Path,
) -> std::collections::HashMap<String, (Vec<String>, Vec<String>)> {
    let Some(graph) = crate::analyzer::imports::build_graph(project_path) else {
        return std::collections::HashMap::new();
    };

    let mut map: std::collections::HashMap<String, (Vec<String>, Vec<String>)> =
        std::collections::HashMap::new();

    for edge in &graph.edges {
        if edge.is_external {
            continue;
        }
        map.entry(edge.from.clone())
            .or_default()
            .0
            .push(edge.to.clone());

        map.entry(edge.to.clone())
            .or_default()
            .1
            .push(edge.from.clone());
    }

    map
}

// ── Dependency propagation ──────────────────────────────────

/// Given a set of changed files (relative paths), return all files that
/// directly import any of them. Used so a file's embedding stays accurate
/// when one of its dependencies changes.
///
/// Returns an empty set when `build_graph` can't determine a language,
/// when no files import the changed set, or when every importer is already
/// in `changed_files`.
pub fn find_dependents(
    project_path: &Path,
    changed_files: &[String],
) -> std::collections::HashSet<String> {
    let Some(graph) = crate::analyzer::imports::build_graph(project_path) else {
        return std::collections::HashSet::new();
    };

    let changed_set: std::collections::HashSet<&str> =
        changed_files.iter().map(|s| s.as_str()).collect();

    graph
        .edges
        .iter()
        .filter(|edge| !edge.is_external && changed_set.contains(edge.to.as_str()))
        .map(|edge| edge.from.clone())
        .filter(|dep| !changed_set.contains(dep.as_str()))
        .collect()
}

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
/// `git_base` enables git-diff-based change detection (e.g. "HEAD", "HEAD~1", "main").
pub fn index_project_background(
    project: &Project,
    force: bool,
    git_base: Option<String>,
) -> String {
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
        let result = index_project(
            &project_clone,
            force,
            git_base.as_deref(),
            |processed, total| {
                update_job(
                    &key,
                    IndexJobStatus::Running {
                        files_processed: processed,
                        files_total: total,
                    },
                );
            },
        );

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

/// Result of the parallel read+hash+chunk phase for a single file.
struct PreparedFile {
    file_rel: String,
    file_hash: String,
    mtime: f64,
    chunks: Vec<Chunk>,
}

/// Decide whether a file should be skipped during incremental indexing.
fn should_skip(
    file_rel: &str,
    file_hash: &str,
    mtime: f64,
    force: bool,
    git_changed: &Option<std::collections::HashSet<String>>,
    existing_hashes: &HashMap<String, String>,
    existing_timestamps: &HashMap<String, f64>,
) -> bool {
    if force {
        return false;
    }
    // Git says unchanged AND we already indexed it.
    if let Some(git_files) = git_changed
        && !git_files.contains(file_rel)
        && existing_timestamps.contains_key(file_rel)
    {
        return true;
    }
    // Hash match — identical content.
    let hash_str = file_hash.to_string();
    if !file_hash.is_empty() && existing_hashes.get(file_rel) == Some(&hash_str) {
        return true;
    }
    // Mtime fallback for pre-hash entries without git info.
    if file_hash.is_empty()
        && git_changed.is_none()
        && let Some(prev) = existing_timestamps.get(file_rel)
        && mtime <= *prev
    {
        return true;
    }
    false
}

/// Persist prepared chunks into SQLite (must be sequential — SQLite
/// doesn't parallelise writes).
fn persist_chunks(
    conn: &rusqlite::Connection,
    prepared: &[PreparedFile],
) -> Result<Vec<Chunk>, String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut all_new = Vec::new();
    for prep in prepared {
        let _ = tx.execute("DELETE FROM chunks WHERE file_path = ?1", [&prep.file_rel]);
        for chunk in &prep.chunks {
            tx.execute(
                "INSERT INTO chunks (file_path, line_start, line_end, text, mtime, file_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    chunk.file_path,
                    chunk.line_start,
                    chunk.line_end,
                    chunk.text,
                    prep.mtime,
                    prep.file_hash,
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        all_new.extend(prep.chunks.clone());
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(all_new)
}

/// Build HNSW index from embeddings, dump to disk atomically, and
/// invalidate the in-memory cache.
fn build_and_save_hnsw(
    all_embeddings: &[Vec<f32>],
    hnsw_path: &Path,
    project: &Project,
) -> Result<(), String> {
    let hnsw: Hnsw<f32, DistCosine> = Hnsw::new(
        HNSW_MAX_CONN,
        all_embeddings.len(),
        HNSW_MAX_LAYERS,
        HNSW_EF_CONSTRUCTION,
        DistCosine,
    );
    let data: Vec<(&Vec<f32>, usize)> = all_embeddings
        .iter()
        .enumerate()
        .map(|(id, emb)| (emb, id))
        .collect();
    hnsw.parallel_insert(&data);

    let tmp_path = hnsw_path.with_extension("_building");
    let _ = std::fs::create_dir_all(&tmp_path);
    hnsw.file_dump(&tmp_path, "index")
        .map_err(|e| format!("Failed to save HNSW index: {}", e))?;
    if hnsw_path.exists() {
        let _ = std::fs::remove_dir_all(hnsw_path);
    }
    std::fs::rename(&tmp_path, hnsw_path)
        .map_err(|e| format!("Failed to finalize HNSW index: {}", e))?;
    invalidate_hnsw_cache(project);
    Ok(())
}

/// Maximum simultaneous indexing operations. Prevents rayon + ONNX + HNSW
/// from saturating CPU and RAM when multiple projects index concurrently.
static ACTIVE_INDEXING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const MAX_CONCURRENT_INDEXING: usize = 2;

/// RAII guard that decrements ACTIVE_INDEXING on drop (even on panic).
struct IndexGuard;
impl Drop for IndexGuard {
    fn drop(&mut self) {
        ACTIVE_INDEXING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Index a project's codebase. `force` re-indexes everything.
/// `git_base` uses `git diff` against a ref (e.g. "HEAD~1") to select changed
/// files — faster and more accurate than mtime after checkout/stash/pull.
/// `progress` callback receives (files_processed, total_files).
///
/// The heavy work (file I/O, SHA-256, chunking) runs in parallel via rayon;
/// only the SQLite persist phase is sequential. At most
/// `MAX_CONCURRENT_INDEXING` indexing jobs run simultaneously.
pub fn index_project(
    project: &Project,
    force: bool,
    git_base: Option<&str>,
    progress: impl Fn(usize, usize) + Sync,
) -> Result<IndexStats, String> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Wait for a slot if too many indexing jobs are active
    loop {
        let current = ACTIVE_INDEXING.load(Ordering::SeqCst);
        if current < MAX_CONCURRENT_INDEXING
            && ACTIVE_INDEXING
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let _guard = IndexGuard;

    let project_path = PathBuf::from(strip_win_prefix(&project.path));
    let files = collect_indexable_files(&project_path);
    let total = files.len();
    if total == 0 {
        return Err("No indexable files found in project".to_string());
    }

    // Setup dirs + DB
    let idx_dir = index_dir(project);
    let _ = std::fs::create_dir_all(&idx_dir);
    let hnsw_path = hnsw_dir(project);
    let _ = std::fs::create_dir_all(&hnsw_path);
    let conn = db::open_meta_db(project)?;

    // Load cached state
    let (mut existing_timestamps, mut existing_hashes) = if !force {
        (
            db::load_file_timestamps(&conn)?,
            db::load_file_hashes(&conn)?,
        )
    } else {
        conn.execute_batch("DELETE FROM chunks;")
            .map_err(|e| e.to_string())?;
        (HashMap::new(), HashMap::new())
    };

    // Cleanup stale entries
    if !force {
        cleanup_stale_chunks(
            &conn,
            &files,
            &mut existing_timestamps,
            &mut existing_hashes,
        )?;
    }

    // Git-based change detection
    let git_changed: Option<std::collections::HashSet<String>> = if !force {
        git_base
            .map(|base| get_git_changed_files(&project_path, base))
            .filter(|c| !c.is_empty())
            .map(|c| c.into_iter().collect())
    } else {
        None
    };

    // Propagate changes to one-hop importers
    if !force {
        let changed: Vec<String> = files
            .iter()
            .filter(|f| {
                if let Some(ref g) = git_changed
                    && g.contains(f.as_str())
                {
                    return true;
                }
                let h = file_sha256(&project_path.join(f));
                !h.is_empty() && existing_hashes.get(f.as_str()) != Some(&h)
            })
            .cloned()
            .collect();
        if !changed.is_empty() {
            for dep in find_dependents(&project_path, &changed) {
                existing_hashes.remove(dep.as_str());
                existing_timestamps.remove(dep.as_str());
            }
        }
    }

    // ── Phase A: parallel read + hash + chunk ────────────────
    let counter = AtomicUsize::new(0);
    let prepared: Vec<PreparedFile> = files
        .par_iter()
        .filter_map(|file_rel| {
            let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
            progress(done, total);

            let abs = project_path.join(file_rel);
            let file_hash = file_sha256(&abs);
            let mtime = file_mtime(&abs);

            if should_skip(
                file_rel,
                &file_hash,
                mtime,
                force,
                &git_changed,
                &existing_hashes,
                &existing_timestamps,
            ) {
                return None;
            }

            let content = std::fs::read_to_string(&abs).ok()?;
            let chunks = chunk_file(file_rel, &content);
            if chunks.is_empty() {
                return None;
            }
            Some(PreparedFile {
                file_rel: file_rel.clone(),
                file_hash,
                mtime,
                chunks,
            })
        })
        .collect();

    // Skipped files = total - prepared (for cached embeddings)
    let skipped_files: Vec<String> = files
        .iter()
        .filter(|f| !prepared.iter().any(|p| p.file_rel == **f))
        .cloned()
        .collect();

    // ── Phase B: sequential DB persist ──────────────────────
    let mut new_chunks = persist_chunks(&conn, &prepared)?;

    // Load cached embeddings for unchanged files
    let mut all_chunks: Vec<Chunk> = Vec::new();
    let mut all_embeddings: Vec<Vec<f32>> = Vec::new();
    if !force && !skipped_files.is_empty() {
        let cached = db::load_chunks_with_embeddings(&conn, &skipped_files)?;
        for (chunk, embedding) in cached {
            all_chunks.push(chunk);
            all_embeddings.push(embedding);
        }
    }

    // Enrich new chunks with structural import context
    if !new_chunks.is_empty() {
        let import_context = build_import_context(&project_path);
        for chunk in &mut new_chunks {
            if let Some((imports, imported_by)) = import_context.get(&chunk.file_path) {
                enrich_chunk_with_context(chunk, imports, imported_by);
            }
        }
    }

    // Generate embeddings for new/modified chunks
    if !new_chunks.is_empty() {
        let new_texts: Vec<String> = new_chunks.iter().map(|c| c.text.clone()).collect();
        let mut new_embeddings: Vec<Vec<f32>> = Vec::with_capacity(new_texts.len());
        for batch in new_texts.chunks(64) {
            new_embeddings.extend(embed_texts(batch)?);
        }
        db::save_embeddings(&conn, &new_chunks, &new_embeddings)?;
        all_chunks.extend(new_chunks);
        all_embeddings.extend(new_embeddings);
    }

    let chunks_total = all_chunks.len();
    if chunks_total == 0 {
        return Err("No code chunks generated from project files".to_string());
    }

    // Build + save HNSW
    build_and_save_hnsw(&all_embeddings, &hnsw_path, project)?;
    db::save_chunk_order(&conn, &all_chunks)?;

    // Stats
    let size_mb = dir_size_mb(&idx_dir);
    let stats = IndexStats {
        files_indexed: total,
        chunks_total,
        model: "BAAI/bge-small-en-v1.5".to_string(),
        size_mb,
        created_at: Utc::now(),
    };
    save_stats(&conn, &stats)?;
    crate::stats::record_saving(crate::stats::TokenSavingsRecord {
        timestamp: Utc::now(),
        project: project.name.clone(),
        operation: "vector_index".to_string(),
        lines_original: chunks_total * CHUNK_LINES,
        lines_filtered: chunks_total,
        savings_pct: 0.0,
    });

    Ok(stats)
}

// ── File collection ─────────────────────────────────────────

/// Source code extensions worth indexing.
/// Source-code extensions indexed for semantic search.
///
/// Deliberately excludes config/data files (`json`, `yaml`, `yml`, `toml`)
/// and shell-like scripts (`sh`, `bash`, `zsh`, `fish`, `ps1`, `bat`,
/// `cmd`) — they dominate the index with package-lock / schema / CI
/// boilerplate that drowns real code in search results. Kept: source
/// languages, `md` for READMEs, `proto`/`sql` for typed code, and the
/// `dockerfile` build spec.
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
    "proto",
    "sql",
    "md",
    "dockerfile",
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
