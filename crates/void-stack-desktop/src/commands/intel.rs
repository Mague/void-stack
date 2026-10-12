//! Intelligence commands exposed to the desktop frontend.
//!
//! Thin wrappers over `void-stack-core` for the redesigned Intelligence
//! zone, the topbar vitals and the pulse line. The heavy ones (review,
//! suggest-tests, dead-code, graph build) run on a blocking thread via
//! `spawn_blocking` so a 2–4 s analysis never freezes the UI thread.

use serde::Serialize;

use void_stack_core::deadcode::DeadCodeReport;
use void_stack_core::global_config::load_global_config;
use void_stack_core::model::Project;
use void_stack_core::review::ReviewPayload;
use void_stack_core::runner::local::strip_win_prefix;
use void_stack_core::testing::TestSuggestions;

/// Resolve a project by name from the global config (owned clone so it can
/// move into a blocking task).
fn project_by_name(name: &str) -> Result<Project, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    config
        .projects
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .cloned()
        .ok_or_else(|| format!("project '{}' not found", name))
}

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("task join error: {}", e))?
}

/// Build (or rebuild) the structural call graph for a project. Required by
/// review_diff, suggest_tests, find_dead_code and the log impact action —
/// none of those work until the graph exists. Returns the build timestamp.
#[tauri::command]
pub async fn build_structural_graph_cmd(
    project: String,
    force: Option<bool>,
) -> Result<String, String> {
    let proj = project_by_name(&project)?;
    let force = force.unwrap_or(false);
    blocking(move || {
        void_stack_core::structural::build_structural_graph(&proj, force)
            .map(|stats| stats.built_at.to_rfc3339())
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn review_diff_cmd(
    project: String,
    git_base: Option<String>,
) -> Result<ReviewPayload, String> {
    let proj = project_by_name(&project)?;
    blocking(move || void_stack_core::review::review_diff(&proj, git_base.as_deref())).await
}

#[tauri::command]
pub async fn suggest_tests_cmd(
    project: String,
    git_base: Option<String>,
    max: Option<usize>,
) -> Result<TestSuggestions, String> {
    let proj = project_by_name(&project)?;
    let max = max.unwrap_or(10);
    blocking(move || {
        void_stack_core::testing::suggest_tests_for_diff(&proj, git_base.as_deref(), max)
    })
    .await
}

#[tauri::command]
pub async fn find_dead_code_cmd(
    project: String,
    max: Option<usize>,
) -> Result<DeadCodeReport, String> {
    let proj = project_by_name(&project)?;
    let max = max.unwrap_or(50);
    blocking(move || void_stack_core::deadcode::find_dead_code(&proj, max)).await
}

/// Freshness timestamps for the topbar vitals: the semantic index
/// `created_at` and the structural graph build time (DB file mtime — read
/// only, never triggers a rebuild). Both RFC3339, `None` when absent.
#[derive(Serialize)]
pub struct ProjectVitals {
    pub index_created_at: Option<String>,
    pub graph_built_at: Option<String>,
}

#[tauri::command]
pub async fn get_project_vitals_cmd(project: String) -> Result<ProjectVitals, String> {
    let proj = project_by_name(&project)?;
    blocking(move || {
        let index_created_at = void_stack_core::vector_index::get_index_stats(&proj)
            .ok()
            .flatten()
            .map(|s| s.created_at.to_rfc3339());

        let graph_path = void_stack_core::structural::graph::structural_db_path(&proj);
        let graph_built_at = std::fs::metadata(&graph_path)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());

        Ok(ProjectVitals {
            index_created_at,
            graph_built_at,
        })
    })
    .await
}

/// Cheap change stamps for the selected project, polled by the UI so views
/// refresh when something changes *outside* the window — a task moved by
/// the MCP server or another editor, a commit from the terminal, a
/// re-index by the file watcher. Every field is a plain `stat`/small file
/// read (no git process, no DB open); the UI only compares them for
/// equality.
#[derive(Serialize, Default, PartialEq, Debug)]
pub struct ProjectFingerprint {
    /// `BOARD.md` mtime + size.
    pub board: String,
    /// Branch ref + commit HEAD points at.
    pub git: String,
    /// Semantic index version (mtime of its `index.version` stamp).
    pub index: String,
    /// Structural graph DB mtime.
    pub graph: String,
}

fn stat_stamp(path: &std::path::Path) -> String {
    std::fs::metadata(path)
        .ok()
        .map(|m| {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or_default();
            format!("{}:{}", mtime, m.len())
        })
        .unwrap_or_default()
}

fn project_fingerprint(proj: &Project) -> ProjectFingerprint {
    let root = std::path::PathBuf::from(strip_win_prefix(&proj.path));
    ProjectFingerprint {
        board: stat_stamp(&void_stack_core::board::board_path(&root)),
        git: void_stack_core::git_util::head_fingerprint(&root).unwrap_or_default(),
        index: void_stack_core::vector_index::index_version(proj)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        graph: stat_stamp(&void_stack_core::structural::graph::structural_db_path(
            proj,
        )),
    }
}

#[tauri::command]
pub async fn get_project_fingerprint_cmd(project: String) -> Result<ProjectFingerprint, String> {
    let proj = project_by_name(&project)?;
    blocking(move || Ok(project_fingerprint(&proj))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support;

    #[test]
    fn test_project_by_name_found_and_missing() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Known", dir.path()));

        // Case-insensitive match.
        let found = project_by_name("known").unwrap();
        assert_eq!(found.name, "Known");

        // Unknown project errors.
        assert!(project_by_name("Ghost").is_err());
    }

    #[test]
    fn test_project_fingerprint_moves_with_board_and_git() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let proj = test_support::project("Fingerprint", root);

        let empty = project_fingerprint(&proj);
        assert_eq!(empty.board, "", "no BOARD.md yet");
        assert_eq!(empty.git, "", "not a git repo");

        std::fs::write(root.join("BOARD.md"), "## Backlog\n").unwrap();
        let with_board = project_fingerprint(&proj);
        assert_ne!(with_board.board, empty.board);

        std::fs::write(root.join("BOARD.md"), "## Backlog\n- [ ] T-1 task\n").unwrap();
        assert_ne!(
            project_fingerprint(&proj).board,
            with_board.board,
            "editing the board changes the stamp (size differs even within one mtime tick)"
        );

        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git").join("HEAD"), "abc123\n").unwrap();
        assert_eq!(project_fingerprint(&proj).git, "abc123");
    }
}
