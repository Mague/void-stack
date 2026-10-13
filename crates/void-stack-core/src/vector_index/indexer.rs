//! Project indexing: background jobs, file collection, chunking and embedding.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::error::IndexError;

use super::chunker::{CHUNK_LINES, Chunk, chunk_file, enrich_chunk_with_context};
use super::db;
use super::search::invalidate_index_cache;
use super::stats::{
    IndexStats, dir_size_mb, file_mtime, file_sha256, get_git_changed_files, index_dir,
    index_version_file, legacy_hnsw_dir, save_stats, touch_index_version,
};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

mod embed;
mod files;
mod jobs;
mod watch;

#[cfg(test)]
use embed::{DEFAULT_EMBED_BATCH_SIZE, length_order, parse_embed_batch};
use embed::{embed_batch_size, embed_sorted_by_length};
#[cfg(test)]
pub(crate) use files::CODE_EXTENSIONS;
#[cfg(test)]
use files::MAX_FILE_SIZE;
pub(crate) use files::collect_indexable_files;
pub(crate) use jobs::indexing_rayon_threads;
use jobs::{IndexGuard, indexing_pool};
pub use jobs::{
    IndexJobStatus, get_index_job_status, index_project_background,
    index_project_background_then_structural,
};
#[cfg(test)]
pub(crate) use jobs::{job_registry, read_job, update_job};
pub use watch::{install_git_hook, is_watching, unwatch_project, watch_project};

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
) -> Result<usize, IndexError> {
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
    let tx = conn.unchecked_transaction().map_err(IndexError::from)?;
    for file in &stale {
        let _ = tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_path = ?1)",
            [file],
        );
        tx.execute("DELETE FROM chunks WHERE file_path = ?1", [file])
            .map_err(IndexError::from)?;
        existing_timestamps.remove(file);
        existing_hashes.remove(file);
    }
    // Drop `chunk_order` rows whose `chunk_id` no longer exists in `chunks`
    // (the table only mattered to the pre-0.33 HNSW index; keep it tidy).
    tx.execute(
        "DELETE FROM chunk_order WHERE chunk_id NOT IN (SELECT id FROM chunks)",
        [],
    )
    .map_err(IndexError::from)?;
    tx.commit().map_err(IndexError::from)?;
    Ok(stale.len())
}

// ── Import context for embeddings ───────────────────────────

/// Build a lightweight import map for the project used to enrich chunk
/// text before embedding. Maps file → (imports, imported_by) where both
/// sides are relative paths within the project. External edges are skipped.
/// Returns an empty map when `build_graph` can't determine a language.
#[cfg(test)]
fn build_import_context(
    project_path: &Path,
) -> std::collections::HashMap<String, (Vec<String>, Vec<String>)> {
    import_context_from(crate::analyzer::imports::build_graph(project_path).as_ref())
}

fn import_context_from(
    graph: Option<&crate::analyzer::graph::DependencyGraph>,
) -> std::collections::HashMap<String, (Vec<String>, Vec<String>)> {
    let Some(graph) = graph else {
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

/// The import-context header of every file in the graph (what its chunks
/// are embedded with).
fn context_headers(
    graph: Option<&crate::analyzer::graph::DependencyGraph>,
) -> std::collections::HashMap<String, String> {
    import_context_from(graph)
        .into_iter()
        .map(|(file, (imports, imported_by))| {
            let h = super::chunker::context_header(&imports, &imported_by);
            (file, h)
        })
        .collect()
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
    dependents_from(
        crate::analyzer::imports::build_graph(project_path).as_ref(),
        changed_files,
    )
}

fn dependents_from(
    graph: Option<&crate::analyzer::graph::DependencyGraph>,
    changed_files: &[String],
) -> std::collections::HashSet<String> {
    let Some(graph) = graph else {
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
) -> Result<Vec<Chunk>, IndexError> {
    let tx = conn.unchecked_transaction().map_err(IndexError::from)?;
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
            .map_err(IndexError::from)?;
        }
        all_new.extend(prep.chunks.clone());
    }
    tx.commit().map_err(IndexError::from)?;

    // Keep the lexical index in sync with the rows just replaced.
    for prep in prepared {
        super::db::fts_replace_file(conn, &prep.file_rel)?;
    }
    Ok(all_new)
}

/// Run a filesystem operation, retrying a few times on failure (transient
/// sharing violations on Windows). Returns the last error.
fn retry_fs(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    const ATTEMPTS: u32 = 5;
    let mut last = Ok(());
    for attempt in 0..ATTEMPTS {
        last = op();
        if last.is_ok() {
            return last;
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    last
}

/// Index a project's codebase. `force` re-indexes everything.
/// `git_base` uses `git diff` against a ref (e.g. "HEAD~1") to select changed
/// files — faster and more accurate than mtime after checkout/stash/pull.
/// `progress` callback receives (files_processed, total_files).
///
/// The heavy work (file I/O, SHA-256, chunking) runs in parallel via rayon;
/// only the SQLite persist phase is sequential. At most
/// `MAX_CONCURRENT_INDEXING` indexing jobs run simultaneously.
/// The files an index run would read (respecting .voidignore /
/// .claudeignore).
pub(crate) fn collect_indexable_files_pub(project_path: &Path) -> Vec<String> {
    collect_indexable_files(project_path)
}

pub fn index_project(
    project: &Project,
    force: bool,
    git_base: Option<&str>,
    progress: impl Fn(usize, usize) + Sync,
) -> Result<IndexStats, IndexError> {
    let _slot = IndexGuard::acquire();

    let project_path = PathBuf::from(strip_win_prefix(&project.path));
    let files = collect_indexable_files(&project_path);
    let total = files.len();
    if total == 0 {
        return Err(IndexError::Other(
            "no indexable files found in project".to_string(),
        ));
    }

    // Force means "from scratch": drop the whole previous index — chunks,
    // FTS, cached embeddings, communities — not just the chunk
    // rows (stale community ids used to survive a forced rebuild). Done
    // after taking the indexing slot so it never pulls the rug from under
    // a job that is still writing.
    if force {
        super::stats::delete_index(project)?;
    }
    let idx_dir = index_dir(project);
    let _ = std::fs::create_dir_all(&idx_dir);
    let conn = db::open_meta_db(project)?;

    // What the previous run stored, minus files that no longer exist.
    let mut known = KnownFiles::default();
    let mut stale_removed = 0;
    if force {
        conn.execute_batch("DELETE FROM chunks; DELETE FROM chunks_fts;")
            .map_err(IndexError::from)?;
    } else {
        known.timestamps = db::load_file_timestamps(&conn)?;
        known.hashes = db::load_file_hashes(&conn)?;
        stale_removed =
            cleanup_stale_chunks(&conn, &files, &mut known.timestamps, &mut known.hashes)?;
    }

    let current_hashes = hash_files(&project_path, &files);

    // The import graph is a full parse of the project: build it at most
    // once per run (dependency propagation and chunk enrichment both use
    // it; each used to rebuild it).
    let import_graph: std::cell::OnceCell<Option<crate::analyzer::graph::DependencyGraph>> =
        std::cell::OnceCell::new();
    let graph = || {
        import_graph
            .get_or_init(|| crate::analyzer::imports::build_graph(&project_path))
            .as_ref()
    };

    let git_changed: Option<HashSet<String>> = git_base
        .filter(|_| !force)
        .map(|base| get_git_changed_files(&project_path, base))
        .filter(|c| !c.is_empty())
        .map(|c| c.into_iter().collect());

    // A changed file can change how its one-hop importers read: re-embed
    // them too. And a file's chunks are embedded with its import context
    // ("Used by" / "Imports"): when that context changed — an importer was
    // added or deleted — the file must be re-embedded even though its own
    // content is the same (an incremental run used to keep the stale
    // context; caught by the incremental == full rebuild test).
    if !force {
        let changed = changed_files(&files, &git_changed, &current_hashes, &known.hashes);
        let added = files.iter().any(|f| !known.hashes.contains_key(f));
        if !changed.is_empty() {
            for dep in dependents_from(graph(), &changed) {
                known.forget(&dep);
            }
        }
        let stored = db::load_file_contexts(&conn).unwrap_or_default();
        // Indexes built before 0.35 have no stored contexts: don't treat
        // that as "every context changed" (a full re-embed on the first
        // small edit) — this run just records them.
        if !stored.is_empty() && (!changed.is_empty() || added || stale_removed > 0) {
            let now = context_headers(graph());
            for f in &files {
                let before = stored.get(f).map(|s| s.as_str()).unwrap_or("");
                let after = now.get(f).map(|s| s.as_str()).unwrap_or("");
                if before != after {
                    known.forget(f);
                }
            }
        }
    }

    let prepared = prepare_files(
        &project_path,
        &files,
        &current_hashes,
        &PrepareOptions {
            force,
            git_changed: &git_changed,
            known: &known,
        },
        &progress,
    );

    // Nothing changed since the last run (a save that didn't alter content,
    // a watcher burst, a no-op hook): keep everything as it is.
    if !force
        && prepared.is_empty()
        && stale_removed == 0
        && index_version_file(project).is_file()
        && let Ok(mut existing) = super::stats::load_stats(&conn)
    {
        // Up to date as of now, even though nothing was rewritten.
        existing.updated_at = Some(Utc::now());
        let _ = super::stats::save_stats(&conn, &existing);
        return Ok(existing);
    }

    let mut new_chunks = persist_chunks(&conn, &prepared)?;
    drop(prepared);
    embed_new_chunks(&conn, &mut new_chunks, graph())?;
    db::save_file_contexts(&conn, &context_headers(graph()))?;
    let stats = finish_index(project, &conn, total, &idx_dir)?;
    // Communities used to exist only if someone ran cluster_project by
    // hand (nobody did): keep them current on every index run.
    super::cluster::refresh_after_index(project, &conn, force);
    Ok(stats)
}

/// Timestamps and content hashes the previous run stored per file.
#[derive(Default)]
struct KnownFiles {
    timestamps: HashMap<String, f64>,
    hashes: HashMap<String, String>,
}

impl KnownFiles {
    /// Treat `file` as never indexed, so it is re-chunked and re-embedded.
    fn forget(&mut self, file: &str) {
        self.hashes.remove(file);
        self.timestamps.remove(file);
    }
}

/// SHA-256 of every file, once per run, in parallel on the indexing pool.
fn hash_files(root: &Path, files: &[String]) -> HashMap<String, String> {
    use rayon::prelude::*;
    indexing_pool().install(|| {
        files
            .par_iter()
            .map(|f| (f.clone(), file_sha256(&root.join(f))))
            .collect()
    })
}

/// Files whose content differs from the last run, or that git reports as
/// changed.
fn changed_files(
    files: &[String],
    git_changed: &Option<HashSet<String>>,
    current_hashes: &HashMap<String, String>,
    known_hashes: &HashMap<String, String>,
) -> Vec<String> {
    files
        .iter()
        .filter(|f| {
            git_changed.as_ref().is_some_and(|g| g.contains(f.as_str()))
                || current_hashes
                    .get(f.as_str())
                    .is_some_and(|h| !h.is_empty() && known_hashes.get(f.as_str()) != Some(h))
        })
        .cloned()
        .collect()
}

struct PrepareOptions<'a> {
    force: bool,
    git_changed: &'a Option<HashSet<String>>,
    known: &'a KnownFiles,
}

/// Phase A: read and chunk every file that needs (re-)indexing, in
/// parallel on the indexing-only rayon pool so file I/O and chunking don't
/// compete with other rayon work on the global pool.
fn prepare_files(
    root: &Path,
    files: &[String],
    current_hashes: &HashMap<String, String>,
    opts: &PrepareOptions,
    progress: &(impl Fn(usize, usize) + Sync),
) -> Vec<PreparedFile> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let total = files.len();
    let counter = AtomicUsize::new(0);
    indexing_pool().install(|| {
        files
            .par_iter()
            .filter_map(|file_rel| {
                let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                progress(done, total);

                let abs = root.join(file_rel);
                let file_hash = current_hashes.get(file_rel).cloned().unwrap_or_default();
                let mtime = file_mtime(&abs);
                if should_skip(
                    file_rel,
                    &file_hash,
                    mtime,
                    opts.force,
                    opts.git_changed,
                    &opts.known.hashes,
                    &opts.known.timestamps,
                ) {
                    return None;
                }

                let content = crate::fs_util::read_source(&abs)?;
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
            .collect()
    })
}

/// Add each new chunk's import context, then embed and store only the new
/// or modified chunks. Unchanged files keep the embeddings already in
/// `meta.db` — searches read them from there.
fn embed_new_chunks(
    conn: &rusqlite::Connection,
    new_chunks: &mut [Chunk],
    graph: Option<&crate::analyzer::graph::DependencyGraph>,
) -> Result<(), IndexError> {
    if new_chunks.is_empty() {
        return Ok(());
    }
    let import_context = import_context_from(graph);
    for chunk in new_chunks.iter_mut() {
        if let Some((imports, imported_by)) = import_context.get(&chunk.file_path) {
            enrich_chunk_with_context(chunk, imports, imported_by);
        }
    }
    let new_embeddings = embed_sorted_by_length(new_chunks, embed_batch_size())?;
    db::save_embeddings(conn, new_chunks, &new_embeddings)
}

/// Stamp the new index version, drop pre-0.33 leftovers, save stats and
/// record the token savings.
fn finish_index(
    project: &Project,
    conn: &rusqlite::Connection,
    files_indexed: usize,
    idx_dir: &Path,
) -> Result<IndexStats, IndexError> {
    let chunks_total = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE embedding IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map_err(IndexError::from)? as usize;
    if chunks_total == 0 {
        return Err(IndexError::Other(
            "no code chunks generated from project files".to_string(),
        ));
    }

    // Indexes built before 0.33 also carry an HNSW graph and the id
    // mapping it needed; neither is read anymore.
    let legacy = legacy_hnsw_dir(project);
    if legacy.exists() {
        let _ = retry_fs(|| std::fs::remove_dir_all(&legacy));
    }
    let _ = conn.execute("DELETE FROM chunk_order", []);
    touch_index_version(project)
        .map_err(|e| IndexError::Other(format!("failed to stamp index version: {}", e)))?;
    invalidate_index_cache(project);

    let stats = IndexStats {
        files_indexed,
        chunks_total,
        model: "BAAI/bge-small-en-v1.5".to_string(),
        size_mb: dir_size_mb(idx_dir),
        created_at: Utc::now(),
        updated_at: Some(Utc::now()),
        model_ready: true,
    };
    save_stats(conn, &stats)?;
    record_index_savings(project, conn, chunks_total);
    Ok(stats)
}

/// Token savings of answering from chunks instead of whole files. Real LOC
/// across all indexed files = max(line_end) per file: the chunker emits
/// contiguous chunks, so that is the file's line count without re-reading
/// it from disk.
fn record_index_savings(project: &Project, conn: &rusqlite::Connection, chunks_total: usize) {
    let lines_original: usize = conn
        .query_row(
            "SELECT COALESCE(SUM(m), 0) FROM \
             (SELECT MAX(line_end) AS m FROM chunks GROUP BY file_path)",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize;
    let lines_filtered: usize = chunks_total * CHUNK_LINES;
    let savings_pct = if lines_original > lines_filtered {
        ((1.0 - lines_filtered as f64 / lines_original as f64) * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    };
    crate::stats::record_saving(crate::stats::TokenSavingsRecord {
        timestamp: Utc::now(),
        project: project.name.clone(),
        operation: "vector_index".to_string(),
        lines_original,
        lines_filtered,
        savings_pct,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;

    #[test]
    fn test_changed_files_by_hash_and_git() {
        let files: Vec<String> = ["a.rs", "b.rs", "c.rs", "d.rs"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let map = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let current = map(&[
            ("a.rs", "h1"),
            ("b.rs", "h2-new"),
            ("c.rs", ""),
            ("d.rs", "h4"),
        ]);
        let known = map(&[("a.rs", "h1"), ("b.rs", "h2"), ("c.rs", "h3")]);
        // b changed content, d was never indexed; c's hash could not be
        // computed (unreadable), which is not a change.
        assert_eq!(
            changed_files(&files, &None, &current, &known),
            vec!["b.rs", "d.rs"]
        );
        // git can flag a file even when its hash matches.
        let git: Option<HashSet<String>> = Some(["a.rs".to_string()].into_iter().collect());
        assert_eq!(
            changed_files(&files, &git, &current, &known),
            vec!["a.rs", "b.rs", "d.rs"]
        );
    }

    /// Build a fixture project rooted at `path`. Naming follows the
    /// `<area>-fixture-<pid>` convention so `void doctor --fix` can sweep
    /// any leaked leftovers; a per-test suffix keeps parallel tests from
    /// sharing the same meta.db.
    fn fixture_project(suffix: &str, path: &Path) -> Project {
        Project {
            name: format!("indexer-fixture-{}-{}", std::process::id(), suffix),
            description: String::new(),
            path: path.to_string_lossy().to_string(),
            project_type: None,
            tags: Vec::new(),
            services: Vec::new(),
            hooks: None,
        }
    }

    /// Normalize collected relative paths to forward slashes (Windows).
    fn norm(paths: Vec<String>) -> HashSet<String> {
        paths.into_iter().map(|p| p.replace('\\', "/")).collect()
    }

    // ── collect_indexable_files ─────────────────────────────

    #[test]
    fn test_collect_filters_by_code_extension() {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in [
            ("main.rs", "fn main() {}"),
            ("app.py", "print('x')"),
            ("game.verse", "OnBegin()"),
            ("README.md", "# readme"),
            ("Dockerfile", "FROM alpine"),
            ("notes.txt", "plain text"),
            ("data.json", "{}"),
            ("run.sh", "echo hi"),
            ("conf.toml", "a = 1"),
            ("deploy.ps1", "Write-Host hi"),
        ] {
            fs::write(dir.path().join(name), content).unwrap();
        }

        let files = norm(collect_indexable_files(dir.path()));
        for expected in ["main.rs", "app.py", "game.verse", "README.md", "Dockerfile"] {
            assert!(files.contains(expected), "missing {}", expected);
        }
        for excluded in [
            "notes.txt",
            "data.json",
            "run.sh",
            "conf.toml",
            "deploy.ps1",
        ] {
            assert!(!files.contains(excluded), "should exclude {}", excluded);
        }
        assert_eq!(files.len(), 5);
    }

    #[test]
    fn test_collect_skips_build_and_unreal_dirs() {
        let dir = tempfile::tempdir().unwrap();
        // Skipped directories, including the Unreal/UEFN generated ones.
        for skip in [
            "node_modules",
            "target",
            "dist",
            "vendor",
            "coverage",
            "Binaries",
            "Intermediate",
            "Saved",
            "DerivedDataCache",
            ".hidden",
        ] {
            let sub = dir.path().join(skip);
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join("code.rs"), "fn skipped() {}").unwrap();
        }
        // Kept directories: Plugins/ (Unreal source plugins) and src/.
        fs::create_dir_all(dir.path().join("Plugins")).unwrap();
        fs::write(dir.path().join("Plugins").join("tool.verse"), "Tool := ...").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("lib.rs"), "pub fn a() {}").unwrap();

        let files = norm(collect_indexable_files(dir.path()));
        let expected: HashSet<String> = ["Plugins/tool.verse", "src/lib.rs"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(files, expected);
    }

    #[test]
    fn test_collect_respects_voidignore_and_claudeignore() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "ignored/\n").unwrap();
        fs::write(dir.path().join(".claudeignore"), "src/secret.rs\n").unwrap();

        fs::create_dir_all(dir.path().join("ignored")).unwrap();
        fs::write(dir.path().join("ignored").join("skipme.rs"), "fn s() {}").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("secret.rs"), "fn h() {}").unwrap();
        fs::write(dir.path().join("src").join("kept.rs"), "fn k() {}").unwrap();

        let files = norm(collect_indexable_files(dir.path()));
        assert!(files.contains("src/kept.rs"));
        assert!(!files.contains("ignored/skipme.rs"));
        assert!(!files.contains("src/secret.rs"));
        // The dotfiles themselves are never indexed (hidden entries).
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_collect_skips_generated_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "proto.pb.rs",
            "service_pb2.py",
            "api.pb.go",
            "model.g.dart",
            "model.freezed.dart",
            "tables.gen.go",
        ] {
            fs::write(dir.path().join(name), "generated").unwrap();
        }
        fs::write(dir.path().join("normal.rs"), "fn real() {}").unwrap();

        let files = norm(collect_indexable_files(dir.path()));
        let expected: HashSet<String> = ["normal.rs".to_string()].into_iter().collect();
        assert_eq!(files, expected);
    }

    #[test]
    fn test_collect_skips_files_over_max_size() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("big.rs"),
            "a".repeat((MAX_FILE_SIZE + 1) as usize),
        )
        .unwrap();
        fs::write(dir.path().join("small.rs"), "fn tiny() {}").unwrap();

        let files = norm(collect_indexable_files(dir.path()));
        assert!(files.contains("small.rs"));
        assert!(!files.contains("big.rs"));
    }

    #[test]
    fn test_collect_respects_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        // 5 directory levels: still within the depth-6 budget.
        let five = dir.path().join("d1/d2/d3/d4/d5");
        fs::create_dir_all(&five).unwrap();
        fs::write(five.join("ok.rs"), "fn ok() {}").unwrap();
        // 6 directory levels: beyond the recursion budget.
        let six = five.join("d6");
        fs::create_dir_all(&six).unwrap();
        fs::write(six.join("deep.rs"), "fn deep() {}").unwrap();
        fs::write(dir.path().join("root.rs"), "fn root() {}").unwrap();

        let files = norm(collect_indexable_files(dir.path()));
        assert!(files.contains("root.rs"));
        assert!(files.contains("d1/d2/d3/d4/d5/ok.rs"));
        assert!(!files.contains("d1/d2/d3/d4/d5/d6/deep.rs"));
    }

    // ── should_skip ─────────────────────────────────────────

    fn skip_fixtures() -> (HashMap<String, String>, HashMap<String, f64>) {
        let hashes = HashMap::from([("a.rs".to_string(), "h1".to_string())]);
        let times = HashMap::from([("a.rs".to_string(), 100.0)]);
        (hashes, times)
    }

    #[test]
    fn test_should_skip_force_never_skips() {
        let (hashes, times) = skip_fixtures();
        assert!(!should_skip(
            "a.rs", "h1", 100.0, true, &None, &hashes, &times
        ));
    }

    #[test]
    fn test_should_skip_on_hash_match() {
        let (hashes, times) = skip_fixtures();
        assert!(should_skip(
            "a.rs", "h1", 999.0, false, &None, &hashes, &times
        ));
        // Different content hash → must re-index.
        assert!(!should_skip(
            "a.rs", "h2", 999.0, false, &None, &hashes, &times
        ));
    }

    #[test]
    fn test_should_skip_git_unchanged_and_already_indexed() {
        let (hashes, times) = skip_fixtures();
        let git: Option<HashSet<String>> = Some(HashSet::from(["b.rs".to_string()]));
        // a.rs: not in git's changed set + already indexed → skip even
        // though its hash differs (git verdict wins).
        assert!(should_skip(
            "a.rs", "h2", 200.0, false, &git, &hashes, &times
        ));
        // b.rs: git says changed and there's no matching hash → re-index.
        assert!(!should_skip(
            "b.rs", "h2", 200.0, false, &git, &hashes, &times
        ));
        // c.rs: git says unchanged but it was never indexed → re-index.
        assert!(!should_skip("c.rs", "", 0.0, false, &git, &hashes, &times));
    }

    #[test]
    fn test_should_skip_mtime_fallback_for_prehash_entries() {
        let (hashes, times) = skip_fixtures();
        // Empty hash + no git info → fall back to mtime comparison.
        assert!(should_skip(
            "a.rs", "", 100.0, false, &None, &hashes, &times
        ));
        assert!(should_skip("a.rs", "", 50.0, false, &None, &hashes, &times));
        assert!(!should_skip(
            "a.rs", "", 150.0, false, &None, &hashes, &times
        ));
    }

    #[test]
    fn test_should_skip_new_file_is_indexed() {
        let (hashes, times) = skip_fixtures();
        assert!(!should_skip(
            "new.rs", "hx", 1.0, false, &None, &hashes, &times
        ));
    }

    // ── cleanup_stale_chunks ────────────────────────────────

    #[test]
    fn test_cleanup_stale_chunks_removes_missing_files() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("cleanup", dir.path());
        let conn = db::open_meta_db(&project).unwrap();

        for file in ["a.rs", "b.rs"] {
            conn.execute(
                "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
                 VALUES (?1, 1, 10, 'fn x() {}', 1.0)",
                [file],
            )
            .unwrap();
        }
        let a_id: i64 = conn
            .query_row("SELECT id FROM chunks WHERE file_path = 'a.rs'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let b_id: i64 = conn
            .query_row("SELECT id FROM chunks WHERE file_path = 'b.rs'", [], |r| {
                r.get(0)
            })
            .unwrap();
        conn.execute(
            "INSERT INTO chunk_order (hnsw_id, chunk_id) VALUES (0, ?1), (1, ?2)",
            rusqlite::params![a_id, b_id],
        )
        .unwrap();

        let mut times: HashMap<String, f64> =
            HashMap::from([("a.rs".to_string(), 1.0), ("b.rs".to_string(), 1.0)]);
        let mut hashes: HashMap<String, String> = HashMap::from([
            ("a.rs".to_string(), "ha".to_string()),
            ("b.rs".to_string(), "hb".to_string()),
        ]);

        // b.rs is gone from the indexable set → its chunks must be dropped.
        let removed =
            cleanup_stale_chunks(&conn, &["a.rs".to_string()], &mut times, &mut hashes).unwrap();
        assert_eq!(removed, 1);
        assert!(times.contains_key("a.rs") && !times.contains_key("b.rs"));
        assert!(hashes.contains_key("a.rs") && !hashes.contains_key("b.rs"));

        let b_chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_path = 'b.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b_chunks, 0);
        // Orphaned chunk_order rows are pruned; a.rs's row survives.
        let order_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunk_order", [], |r| r.get(0))
            .unwrap();
        assert_eq!(order_rows, 1);
    }

    #[test]
    fn test_cleanup_stale_chunks_noop_cases() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("cleanup-noop", dir.path());
        let conn = db::open_meta_db(&project).unwrap();

        // Empty timestamp map → nothing to clean.
        let mut times: HashMap<String, f64> = HashMap::new();
        let mut hashes: HashMap<String, String> = HashMap::new();
        let removed =
            cleanup_stale_chunks(&conn, &["a.rs".to_string()], &mut times, &mut hashes).unwrap();
        assert_eq!(removed, 0);

        // All known files still present → nothing stale.
        times.insert("a.rs".to_string(), 1.0);
        let removed =
            cleanup_stale_chunks(&conn, &["a.rs".to_string()], &mut times, &mut hashes).unwrap();
        assert_eq!(removed, 0);
        assert!(times.contains_key("a.rs"));
    }

    // ── persist_chunks ──────────────────────────────────────

    #[test]
    fn test_persist_chunks_replaces_rows_and_syncs_fts() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("persist", dir.path());
        let conn = db::open_meta_db(&project).unwrap();

        // Pre-existing outdated row for the same file.
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
             VALUES ('x.rs', 1, 5, 'old text', 0.5)",
            [],
        )
        .unwrap();

        let prepared = vec![PreparedFile {
            file_rel: "x.rs".to_string(),
            file_hash: "hash-x".to_string(),
            mtime: 2.0,
            chunks: vec![
                Chunk {
                    file_path: "x.rs".to_string(),
                    text: "fn one() {}".to_string(),
                    line_start: 1,
                    line_end: 40,
                },
                Chunk {
                    file_path: "x.rs".to_string(),
                    text: "fn two() {}".to_string(),
                    line_start: 41,
                    line_end: 80,
                },
            ],
        }];

        let out = persist_chunks(&conn, &prepared).unwrap();
        assert_eq!(out.len(), 2);

        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_path = 'x.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 2);
        let old_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE text = 'old text'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_rows, 0, "stale row must be replaced");
        // Lexical (FTS) mirror stays in sync with the replaced rows.
        let fts_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE file_path = 'x.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_rows, 2);
    }

    // ── Job registry ────────────────────────────────────────

    #[test]
    fn test_job_registry_roundtrip() {
        let key = format!("indexer-fixture-{}-registry", std::process::id());
        assert!(read_job(&key).is_none());

        update_job(
            &key,
            IndexJobStatus::Running {
                files_processed: 3,
                files_total: 10,
            },
        );
        match read_job(&key) {
            Some(IndexJobStatus::Running {
                files_processed,
                files_total,
            }) => {
                assert_eq!(files_processed, 3);
                assert_eq!(files_total, 10);
            }
            other => panic!("expected Running, got {:?}", other),
        }

        update_job(
            &key,
            IndexJobStatus::Failed {
                error: "boom".to_string(),
            },
        );
        match read_job(&key) {
            Some(IndexJobStatus::Failed { error }) => assert_eq!(error, "boom"),
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn test_get_index_job_status_uses_project_path_as_key() {
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("status", dir.path());
        assert!(get_index_job_status(&project).is_none());

        update_job(
            &project.path,
            IndexJobStatus::Running {
                files_processed: 1,
                files_total: 2,
            },
        );
        assert!(matches!(
            get_index_job_status(&project),
            Some(IndexJobStatus::Running { .. })
        ));
    }

    #[test]
    fn test_indexing_rayon_threads_within_bounds() {
        let n = indexing_rayon_threads();
        assert!((2..=6).contains(&n), "thread count out of bounds: {}", n);
    }

    // ── Dependency propagation / import context ─────────────

    fn python_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "").unwrap();
        fs::write(
            dir.path().join("main.py"),
            "import utils\n\nprint(utils.helper())\n",
        )
        .unwrap();
        fs::write(dir.path().join("utils.py"), "def helper():\n    return 1\n").unwrap();
        dir
    }

    #[test]
    fn test_find_dependents_returns_direct_importers() {
        let dir = python_fixture();
        let deps = find_dependents(dir.path(), &["utils.py".to_string()]);
        assert!(deps.contains("main.py"), "deps were {:?}", deps);
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_find_dependents_excludes_already_changed_files() {
        let dir = python_fixture();
        let deps = find_dependents(dir.path(), &["utils.py".to_string(), "main.py".to_string()]);
        assert!(deps.is_empty(), "deps were {:?}", deps);
    }

    #[test]
    fn test_find_dependents_no_importers() {
        let dir = python_fixture();
        // Nothing imports main.py.
        let deps = find_dependents(dir.path(), &["main.py".to_string()]);
        assert!(deps.is_empty(), "deps were {:?}", deps);
    }

    #[test]
    fn test_find_dependents_empty_project_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let deps = find_dependents(dir.path(), &["whatever.py".to_string()]);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_build_import_context_maps_both_directions() {
        let dir = python_fixture();
        let ctx = build_import_context(dir.path());
        let (imports, _) = ctx.get("main.py").expect("main.py missing from context");
        assert!(imports.contains(&"utils.py".to_string()));
        let (_, imported_by) = ctx.get("utils.py").expect("utils.py missing from context");
        assert!(imported_by.contains(&"main.py".to_string()));
    }

    // ── install_git_hook ────────────────────────────────────

    fn hook_path(dir: &Path) -> PathBuf {
        dir.join(".git").join("hooks").join("post-commit")
    }

    #[test]
    fn test_install_git_hook_requires_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("hook-nogit", dir.path());
        let err = install_git_hook(&project).unwrap_err();
        assert!(err.to_string().contains("not a git repository"));
    }

    #[test]
    fn test_install_git_hook_creates_fresh_hook() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git").join("hooks")).unwrap();
        let project = fixture_project("hook-fresh", dir.path());

        install_git_hook(&project).unwrap();
        let content = fs::read_to_string(hook_path(dir.path())).unwrap();
        assert!(content.starts_with("#!/bin/sh"));
        assert!(content.contains(&format!("void index {} --git-base HEAD~1", project.name)));
        assert!(content.contains(&format!("void graph-build {}", project.name)));
    }

    #[test]
    fn test_install_git_hook_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git").join("hooks")).unwrap();
        let project = fixture_project("hook-idem", dir.path());

        install_git_hook(&project).unwrap();
        install_git_hook(&project).unwrap();
        let content = fs::read_to_string(hook_path(dir.path())).unwrap();
        assert_eq!(content.matches("void graph-build").count(), 1);
    }

    #[test]
    fn test_install_git_hook_upgrades_legacy_index_only_hook() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git").join("hooks")).unwrap();
        let project = fixture_project("hook-upgrade", dir.path());
        // Hook written by an older version: index only, no graph-build.
        let legacy = format!(
            "#!/bin/sh\n# Auto-generated by void-stack\n(void index {} --git-base HEAD~1) 2>/dev/null &\n",
            project.name
        );
        fs::write(hook_path(dir.path()), &legacy).unwrap();

        install_git_hook(&project).unwrap();
        let content = fs::read_to_string(hook_path(dir.path())).unwrap();
        assert!(content.contains("void graph-build"));
        // The legacy line is replaced, not duplicated.
        assert_eq!(content.matches("void index").count(), 1);
    }

    #[test]
    fn test_install_git_hook_appends_to_custom_hook() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git").join("hooks")).unwrap();
        let project = fixture_project("hook-append", dir.path());
        fs::write(hook_path(dir.path()), "#!/bin/sh\necho custom-step\n").unwrap();

        install_git_hook(&project).unwrap();
        let content = fs::read_to_string(hook_path(dir.path())).unwrap();
        assert!(content.contains("echo custom-step"), "custom line lost");
        assert!(content.contains("void graph-build"));
    }

    // ── Watch registry ──────────────────────────────────────

    #[test]
    fn test_watch_project_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("watch", dir.path());

        assert!(!is_watching(&project));
        watch_project(&project).unwrap();
        assert!(is_watching(&project));
        unwatch_project(&project);
        assert!(!is_watching(&project));
        // Idempotent: unwatching again is a no-op.
        unwatch_project(&project);
        assert!(!is_watching(&project));
    }

    // ── Incremental == full (xerj-style equivalence) ─────────

    /// Everything an index answers from: chunk rows with their embeddings,
    /// the FTS rows, and the stats counters.
    /// (file, line_start, line_end, text, embedding blob)
    type ChunkRow = (String, i64, i64, String, Vec<u8>);

    fn index_snapshot(project: &Project) -> (Vec<ChunkRow>, i64, usize) {
        let conn = db::open_meta_db(project).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line_start, line_end, text, embedding FROM chunks \
                 ORDER BY file_path, line_start, line_end",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, Option<Vec<u8>>>(4)?.unwrap_or_default(),
                ))
            })
            .unwrap()
            .flatten()
            .collect();
        let fts: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
            .unwrap();
        let stats = super::super::stats::load_stats(&conn).unwrap();
        (rows, fts, stats.chunks_total)
    }

    fn write_tree(root: &Path, files: &[(&str, &str)]) {
        for (rel, text) in files {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        }
    }

    /// Indexing a project in steps (full, then edits + incremental runs)
    /// must leave exactly the index a from-scratch run builds over the same
    /// final files — chunks, text (incl. import context), embeddings, FTS
    /// and counters. This is the class of bug that kept stale indexes
    /// alive before (old rows surviving, context from a deleted import).
    #[test]
    fn test_incremental_index_equals_full_rebuild() {
        crate::isolate_test_data_dir();
        super::super::search::FAKE_EMBEDDINGS.with(|f| f.set(true));

        let initial: &[(&str, &str)] = &[
            ("requirements.txt", "requests\n"),
            ("app/__init__.py", ""),
            (
                "app/api.py",
                "class Request:\n    def __init__(self, rid):\n        self.rid = rid\n\n\ndef handle(rid):\n    return Request(rid)\n",
            ),
            (
                "app/store.py",
                "from app.api import Request\n\n\ndef save(req):\n    return req.rid > 0\n\n\ndef load(rid):\n    return Request(rid)\n",
            ),
            (
                "app/report.py",
                "import json\nfrom app.store import load\n\n\ndef report(rows):\n    return json.dumps([load(r).rid for r in rows])\n",
            ),
            (
                "web/client.ts",
                "import { fetchJson } from './http'\n\nexport async function getUser(id: string) {\n  return fetchJson(`/api/users/${id}`)\n}\n",
            ),
            (
                "web/http.ts",
                "export async function fetchJson(url: string) {\n  const r = await fetch(url)\n  return r.json()\n}\n",
            ),
        ];
        let stepped = tempfile::tempdir().unwrap();
        write_tree(stepped.path(), initial);
        let a = fixture_project("equiv-stepped", stepped.path());
        index_project(&a, true, None, |_, _| {}).unwrap();

        // Edits: change a module others import, delete one, add one, and
        // a second round touching only an importer.
        write_tree(
            stepped.path(),
            &[(
                "app/api.py",
                "class Request:\n    def __init__(self, rid, name):\n        self.rid = rid\n        self.name = name\n\n\ndef handle(rid, name):\n    return Request(rid, name)\n",
            )],
        );
        fs::remove_file(stepped.path().join("app/report.py")).unwrap();
        write_tree(
            stepped.path(),
            &[(
                "app/auth.py",
                "from app.api import Request\n\n\ndef allowed(req):\n    return bool(req.name)\n",
            )],
        );
        index_project(&a, false, None, |_, _| {}).unwrap();
        write_tree(
            stepped.path(),
            &[(
                "web/client.ts",
                "import { fetchJson } from './http'\n\nexport async function getUser(id: string) {\n  return fetchJson(`/api/v2/users/${id}`)\n}\n",
            )],
        );
        index_project(&a, false, None, |_, _| {}).unwrap();
        // Deleting an importer alone: the imported module's "Used by"
        // context must drop it even though that module did not change.
        fs::remove_file(stepped.path().join("app/auth.py")).unwrap();
        index_project(&a, false, None, |_, _| {}).unwrap();
        // A no-op run changes nothing.
        index_project(&a, false, None, |_, _| {}).unwrap();

        // The same final tree, indexed once from scratch.
        let fresh = tempfile::tempdir().unwrap();
        for entry in walk_files(stepped.path()) {
            let rel = entry.strip_prefix(stepped.path()).unwrap();
            let dst = fresh.path().join(rel);
            fs::create_dir_all(dst.parent().unwrap()).unwrap();
            fs::copy(&entry, dst).unwrap();
        }
        let b = fixture_project("equiv-fresh", fresh.path());
        index_project(&b, true, None, |_, _| {}).unwrap();

        let (rows_a, fts_a, total_a) = index_snapshot(&a);
        let (rows_b, fts_b, total_b) = index_snapshot(&b);
        let files = |rows: &[ChunkRow]| {
            rows.iter()
                .map(|r| format!("{}:{}-{}", r.0, r.1, r.2))
                .collect::<Vec<_>>()
        };
        assert_eq!(files(&rows_a), files(&rows_b), "chunk boundaries differ");
        for (x, y) in rows_a.iter().zip(&rows_b) {
            assert_eq!(x.3, y.3, "chunk text differs for {}:{}", x.0, x.1);
            assert_eq!(x.4, y.4, "embedding differs for {}:{}", x.0, x.1);
        }
        assert_eq!(fts_a, fts_b, "FTS rows differ");
        assert_eq!(total_a, total_b, "stats differ");
        assert!(
            !rows_a.iter().any(|r| r.0 == "app/report.py"),
            "deleted file still indexed"
        );
        // The fixture really exercises import context — what incremental
        // runs must keep in sync across files. Context is embedded, not
        // stored: a chunk with context has an embedding that differs from
        // the embedding of its stored text alone.
        let plain = |t: &str| -> Vec<u8> {
            super::super::search::fake_embedding_pub(t)
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect()
        };
        assert!(
            rows_a.iter().any(|r| r.4 != plain(&r.3)),
            "fixture produced no import context"
        );
        super::super::search::FAKE_EMBEDDINGS.with(|f| f.set(false));
    }

    fn walk_files(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
        out
    }

    // ── Background job failure path (no embedding model needed) ──

    #[test]
    fn test_index_project_background_fails_on_empty_project() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("bg-empty", dir.path());

        let key = index_project_background(&project, false, None);
        assert_eq!(key, project.path);

        // The job fails before any embedding work (no indexable files),
        // so this never touches the fastembed model or the network.
        for _ in 0..100 {
            match get_index_job_status(&project) {
                Some(IndexJobStatus::Failed { error }) => {
                    assert!(
                        error.contains("no indexable files"),
                        "unexpected error: {}",
                        error
                    );
                    return;
                }
                Some(IndexJobStatus::Completed { .. }) => {
                    panic!("empty project must not index successfully")
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        panic!("background job did not finish within 10s");
    }

    #[test]
    fn test_parse_embed_batch() {
        assert_eq!(parse_embed_batch(None), DEFAULT_EMBED_BATCH_SIZE);
        assert_eq!(parse_embed_batch(Some("32")), 32);
        assert_eq!(parse_embed_batch(Some("0")), DEFAULT_EMBED_BATCH_SIZE);
        assert_eq!(parse_embed_batch(Some("999")), DEFAULT_EMBED_BATCH_SIZE);
        assert_eq!(parse_embed_batch(Some("x")), DEFAULT_EMBED_BATCH_SIZE);
    }

    #[test]
    fn test_retry_fs_retries_then_succeeds() {
        let mut calls = 0;
        let res = retry_fs(|| {
            calls += 1;
            if calls < 3 {
                Err(std::io::Error::other("busy"))
            } else {
                Ok(())
            }
        });
        assert!(res.is_ok());
        assert_eq!(calls, 3);

        let mut always = 0;
        assert!(
            retry_fs(|| {
                always += 1;
                Err(std::io::Error::other("locked"))
            })
            .is_err()
        );
        assert_eq!(always, 5, "gives up after a bounded number of attempts");
    }

    #[test]
    fn test_length_order_is_stable_ascending() {
        assert_eq!(length_order([30, 5, 30, 1].into_iter()), vec![3, 1, 0, 2]);
        assert!(length_order(std::iter::empty()).is_empty());
    }
}
