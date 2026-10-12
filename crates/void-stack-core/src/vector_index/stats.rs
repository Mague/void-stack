//! Index statistics, paths, and utility helpers.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::error::IndexError;
use crate::model::Project;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub chunks_total: usize,
    pub model: String,
    pub size_mb: f64,
    pub created_at: DateTime<Utc>,
    /// Whether the embedding model can be initialized at this time.
    /// `false` means search will fail even though an index exists.
    /// Defaults to `true` for backward compatibility with existing indexes.
    #[serde(default = "default_model_ready")]
    pub model_ready: bool,
}

fn default_model_ready() -> bool {
    true
}

// ── Paths ───────────────────────────────────────────────────

pub(crate) fn index_dir(project: &Project) -> PathBuf {
    let base = crate::global_config::data_base_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("void-stack").join("indexes").join(&project.name)
}

pub(crate) fn meta_db_path(project: &Project) -> PathBuf {
    index_dir(project).join("meta.db")
}

pub(crate) fn hnsw_dir(project: &Project) -> PathBuf {
    index_dir(project).join("hnsw")
}

/// The graph file `hnsw_rs` writes for basename "index" — its mtime is
/// the index's version stamp. (The cache used to stat `index_data.hnsw`,
/// a file that never exists: the stamp was always 0, so an index rebuilt
/// by *another* process — the MCP server's watcher while the desktop app
/// had it cached — was never reloaded and searches mapped HNSW ids against
/// a stale `chunk_order`.)
pub(crate) fn hnsw_graph_file(project: &Project) -> PathBuf {
    hnsw_dir(project).join("index.hnsw.graph")
}

/// Version stamp of the on-disk semantic index (mtime of the HNSW graph,
/// rewritten on every (re)index), or `None` when there is no index. Cheap
/// enough to poll: a single `stat`, no database open.
pub fn index_version(project: &Project) -> Option<f64> {
    let stamp = file_mtime(&hnsw_graph_file(project));
    (stamp > 0.0).then_some(stamp)
}

/// Cache directory for the fastembed ONNX model.
///
/// Also sets `FASTEMBED_CACHE_DIR` so fastembed respects the same path
/// regardless of its own defaults — the MCP server (launched from an
/// arbitrary working directory) and the desktop app must share it.
pub(crate) fn model_cache_dir() -> PathBuf {
    let base = crate::global_config::data_base_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("void-stack").join("models");
    // SAFETY: written once per process; fastembed reads it at init time.
    unsafe { std::env::set_var("FASTEMBED_CACHE_DIR", &dir) };
    dir
}

// ── Public API ──────────────────────────────────────────────

/// Check if an index exists for a project.
pub fn index_exists(project: &Project) -> bool {
    meta_db_path(project).exists()
}

/// Verify the ONNX embedding model files exist at the expected cache path.
///
/// fastembed stores models in the Hugging Face cache layout, named after the
/// repo it downloads from — `Xenova/bge-small-en-v1.5`, not `BAAI/…` — so
/// the folder is `<cache>/models--Xenova--bge-small-en-v1.5`. The repo name
/// is taken from fastembed itself so it can't drift again (the hardcoded
/// `models--BAAI--…` never existed, so `model_ready` was always `false`).
/// Returns `true` when the model directory exists (regardless of file completeness).
pub fn model_files_exist() -> bool {
    model_cache_dir().join(model_cache_dir_name()).is_dir()
}

/// Hugging Face cache folder name for the embedding model.
fn model_cache_dir_name() -> String {
    let repo = fastembed::TextEmbedding::get_model_info(&fastembed::EmbeddingModel::BGESmallENV15)
        .map(|info| info.model_code.clone())
        .unwrap_or_else(|_| "Xenova/bge-small-en-v1.5".to_string());
    hf_cache_dir_name(&repo)
}

/// `owner/name` → `models--owner--name` (Hugging Face hub cache layout).
fn hf_cache_dir_name(repo: &str) -> String {
    format!("models--{}", repo.replace('/', "--"))
}

/// Get index statistics.
///
/// Checks whether the embedding model files are present on disk so callers
/// can distinguish "no index" from "index exists but search will fail".
pub fn get_index_stats(project: &Project) -> Result<Option<IndexStats>, IndexError> {
    if !index_exists(project) {
        return Ok(None);
    }
    let conn = super::db::open_meta_db(project)?;
    let mut stats = load_stats(&conn)?;
    stats.model_ready = model_files_exist();
    Ok(Some(stats))
}

/// Delete the index for a project.
pub fn delete_index(project: &Project) -> Result<(), IndexError> {
    // Release the in-memory copy too, or searches keep answering from the
    // deleted graph until the process restarts.
    super::search::invalidate_hnsw_cache(project);
    let dir = index_dir(project);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| IndexError::Other(format!("failed to delete index: {}", e)))?;
    }
    Ok(())
}

// ── Stats persistence ──────────────────────────────────────

pub(crate) fn save_stats(conn: &Connection, stats: &IndexStats) -> Result<(), IndexError> {
    let json = serde_json::to_string(stats).map_err(|e| IndexError::MetaDb(e.to_string()))?;
    conn.execute(
        "INSERT OR REPLACE INTO stats (key, value) VALUES ('index_stats', ?1)",
        [&json],
    )
    .map_err(IndexError::from)?;
    Ok(())
}

pub(crate) fn load_stats(conn: &Connection) -> Result<IndexStats, IndexError> {
    let json: String = conn
        .query_row(
            "SELECT value FROM stats WHERE key = 'index_stats'",
            [],
            |row| row.get(0),
        )
        .map_err(IndexError::from)?;
    serde_json::from_str(&json).map_err(|e| IndexError::MetaDb(e.to_string()))
}

// ── Helpers ─────────────────────────────────────────────────

/// Get files changed since a git ref using `git diff` plus uncommitted changes.
/// Returns relative paths (POSIX separator) or an empty vec if not a git repo
/// or git is unavailable. Renamed files resolve to the new path.
pub fn get_git_changed_files(project_path: &Path, base: &str) -> Vec<String> {
    if !project_path.join(".git").exists() {
        return Vec::new();
    }

    let project_arg = project_path.to_string_lossy().to_string();
    use crate::process_util::HideWindow;

    let committed = Command::new("git")
        .args(["-C", &project_arg, "diff", "--name-only", base, "--"])
        .hide_window()
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let unstaged = Command::new("git")
        .args(["-C", &project_arg, "status", "--porcelain"])
        .hide_window()
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.len() > 3)
                .map(|l| {
                    let entry = l[3..].trim();
                    if let Some(new) = entry.split(" -> ").last() {
                        new.to_string()
                    } else {
                        entry.to_string()
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut all: std::collections::HashSet<String> = committed.into_iter().collect();
    all.extend(unstaged);
    all.into_iter().collect()
}

/// Re-export from the consolidated module — used by indexer, tests, and
/// external callers that import `vector_index::stats::file_sha256`.
pub use crate::fs_util::file_sha256;

pub(crate) fn file_mtime(path: &Path) -> f64 {
    path.metadata()
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        })
        .unwrap_or(0.0)
}

pub(crate) fn dir_size_mb(dir: &Path) -> f64 {
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

    fn fixture_project(name: &str, path: &Path) -> Project {
        Project {
            name: format!("{}-fixture-{}", name, std::process::id()),
            path: path.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    fn sh_git(dir: &Path, args: &[&str]) {
        crate::test_support::git(dir, args);
    }

    /// Init a throwaway git repo with identity configured (no signing).
    fn init_git_repo(dir: &Path) {
        sh_git(dir, &["init", "-q"]);
        sh_git(dir, &["config", "user.email", "t@t.io"]);
        sh_git(dir, &["config", "user.name", "t"]);
        sh_git(dir, &["config", "commit.gpgsign", "false"]);
    }

    // ── Paths ───────────────────────────────────────────────

    #[test]
    fn test_index_paths_layout() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("paths", dir.path());

        let idx = index_dir(&project);
        assert!(
            idx.ends_with(Path::new("void-stack").join("indexes").join(&project.name)),
            "index_dir must be <data>/void-stack/indexes/<name>, got {}",
            idx.display()
        );
        assert_eq!(
            meta_db_path(&project),
            idx.join("meta.db"),
            "meta.db lives inside the index dir"
        );
        assert_eq!(
            hnsw_dir(&project),
            idx.join("hnsw"),
            "hnsw dir lives inside the index dir"
        );
        let cache = model_cache_dir();
        assert!(
            cache.ends_with(Path::new("void-stack").join("models")),
            "model cache is <data>/void-stack/models"
        );
        assert_eq!(
            std::env::var("FASTEMBED_CACHE_DIR").ok(),
            Some(cache.to_string_lossy().to_string()),
            "FASTEMBED_CACHE_DIR must match model_cache_dir"
        );
    }

    // ── Stats persistence & index lifecycle ─────────────────

    #[test]
    fn test_stats_roundtrip_and_index_lifecycle() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("stats", dir.path());

        assert!(
            !index_exists(&project),
            "no index must exist before open_meta_db"
        );
        assert!(
            get_index_stats(&project).unwrap().is_none(),
            "get_index_stats must be None without an index"
        );

        let conn = super::super::db::open_meta_db(&project).unwrap();
        assert!(index_exists(&project), "meta.db creation implies existence");

        let stats = IndexStats {
            files_indexed: 12,
            chunks_total: 340,
            model: "test-model".to_string(),
            size_mb: 1.5,
            created_at: Utc::now(),
            model_ready: true,
        };
        save_stats(&conn, &stats).expect("save_stats must succeed");

        let loaded = load_stats(&conn).expect("load_stats must succeed");
        assert_eq!(loaded.files_indexed, 12);
        assert_eq!(loaded.chunks_total, 340);
        assert_eq!(loaded.model, "test-model");
        assert_eq!(loaded.size_mb, 1.5);
        assert_eq!(
            loaded.created_at, stats.created_at,
            "timestamp must round-trip through JSON"
        );

        // Saving again overwrites (INSERT OR REPLACE), no duplicate rows.
        let updated = IndexStats {
            files_indexed: 13,
            ..stats.clone()
        };
        save_stats(&conn, &updated).unwrap();
        assert_eq!(load_stats(&conn).unwrap().files_indexed, 13);

        // Public accessor sees the same stats.
        let via_api = get_index_stats(&project).unwrap().expect("stats exist");
        assert_eq!(via_api.files_indexed, 13);

        // Drop the connection before deleting (Windows holds the file lock).
        drop(conn);
        delete_index(&project).expect("delete_index must succeed");
        assert!(!index_exists(&project), "index gone after delete");
        assert!(
            delete_index(&project).is_ok(),
            "deleting a missing index is a no-op, not an error"
        );
    }

    // ── get_git_changed_files ───────────────────────────────

    #[test]
    fn test_git_changed_files_empty_for_non_repo() {
        let dir = tempfile::tempdir().unwrap();
        let changed = get_git_changed_files(dir.path(), "HEAD");
        assert!(
            changed.is_empty(),
            "a directory without .git must yield no changes"
        );
    }

    #[test]
    fn test_git_changed_files_detects_modified_and_untracked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        std::fs::write(root.join("b.txt"), "two\n").unwrap();
        sh_git(root, &["add", "."]);
        sh_git(root, &["commit", "-q", "-m", "initial"]);

        // Tracked modification (shows in diff vs HEAD and in porcelain).
        std::fs::write(root.join("a.txt"), "one changed\n").unwrap();
        // Untracked file (shows only in porcelain).
        std::fs::write(root.join("c.txt"), "three\n").unwrap();

        let changed = get_git_changed_files(root, "HEAD");
        assert!(
            changed.contains(&"a.txt".to_string()),
            "modified tracked file must be reported, got {:?}",
            changed
        );
        assert!(
            changed.contains(&"c.txt".to_string()),
            "untracked file must be reported, got {:?}",
            changed
        );
        assert!(
            !changed.contains(&"b.txt".to_string()),
            "unchanged file must not be reported, got {:?}",
            changed
        );
        assert_eq!(
            changed.len(),
            2,
            "results are a set: no duplicates for a.txt, got {:?}",
            changed
        );
    }

    #[test]
    fn test_git_changed_files_resolves_renames_to_new_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        std::fs::write(root.join("old.txt"), "content\n").unwrap();
        sh_git(root, &["add", "."]);
        sh_git(root, &["commit", "-q", "-m", "initial"]);
        sh_git(root, &["mv", "old.txt", "new.txt"]);

        let changed = get_git_changed_files(root, "HEAD");
        assert!(
            changed.contains(&"new.txt".to_string()),
            "renamed file must resolve to the new path, got {:?}",
            changed
        );
    }

    // ── File helpers ────────────────────────────────────────

    #[test]
    fn test_file_mtime_positive_for_existing_zero_for_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "data").unwrap();

        assert!(
            file_mtime(&file) > 0.0,
            "existing file must have a positive mtime"
        );
        assert_eq!(
            file_mtime(&dir.path().join("ghost.txt")),
            0.0,
            "missing file must report mtime 0"
        );
    }

    #[test]
    fn test_dir_size_mb_counts_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        // 1 MiB in the root + 0.5 MiB nested = 1.5 MiB total.
        std::fs::write(dir.path().join("big.bin"), vec![0u8; 1_048_576]).unwrap();
        let sub = dir.path().join("nested");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("half.bin"), vec![0u8; 524_288]).unwrap();

        let size = dir_size_mb(dir.path());
        assert!(
            (1.4..1.7).contains(&size),
            "expected ~1.5 MiB including nested files, got {}",
            size
        );

        let empty = tempfile::tempdir().unwrap();
        assert_eq!(dir_size_mb(empty.path()), 0.0, "empty dir is 0 MB");
    }

    #[test]
    fn test_model_cache_dir_name_matches_hf_layout() {
        assert_eq!(
            hf_cache_dir_name("Xenova/bge-small-en-v1.5"),
            "models--Xenova--bge-small-en-v1.5"
        );
        // The name fastembed actually downloads to (what ends up on disk).
        assert_eq!(model_cache_dir_name(), "models--Xenova--bge-small-en-v1.5");
    }
}
