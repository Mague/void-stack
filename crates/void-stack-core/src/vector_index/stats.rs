//! Index statistics, paths, and utility helpers.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::model::Project;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub chunks_total: usize,
    pub model: String,
    pub size_mb: f64,
    pub created_at: DateTime<Utc>,
}

// ── Paths ───────────────────────────────────────────────────

pub(crate) fn index_dir(project: &Project) -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("void-stack").join("indexes").join(&project.name)
}

pub(crate) fn meta_db_path(project: &Project) -> PathBuf {
    index_dir(project).join("meta.db")
}

pub(crate) fn hnsw_dir(project: &Project) -> PathBuf {
    index_dir(project).join("hnsw")
}

pub(crate) fn model_cache_dir() -> PathBuf {
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
    let conn = super::db::open_meta_db(project)?;
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

// ── Stats persistence ──────────────────────────────────────

pub(crate) fn save_stats(conn: &Connection, stats: &IndexStats) -> Result<(), String> {
    let json = serde_json::to_string(stats).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO stats (key, value) VALUES ('index_stats', ?1)",
        [&json],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn load_stats(conn: &Connection) -> Result<IndexStats, String> {
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

/// Get files changed since a git ref using `git diff` plus uncommitted changes.
/// Returns relative paths (POSIX separator) or an empty vec if not a git repo
/// or git is unavailable. Renamed files resolve to the new path.
pub fn get_git_changed_files(project_path: &Path, base: &str) -> Vec<String> {
    if !project_path.join(".git").exists() {
        return Vec::new();
    }

    let project_arg = project_path.to_string_lossy().to_string();

    let committed = Command::new("git")
        .args(["-C", &project_arg, "diff", "--name-only", base, "--"])
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
