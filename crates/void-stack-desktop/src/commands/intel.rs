//! Intelligence commands exposed to the desktop frontend.
//!
//! Thin wrappers over `void-stack-core` for the redesigned Intelligence
//! zone, the topbar vitals and the pulse line. The heavy ones (review,
//! suggest-tests, dead-code, graph build) run on a blocking thread via
//! `spawn_blocking` so a 2–4 s analysis never freezes the UI thread.

use serde::Serialize;

use void_stack_core::deadcode::DeadCodeReport;
use void_stack_core::global_config::load_global_config;
use void_stack_core::review::ReviewPayload;
use void_stack_core::testing::TestSuggestions;

/// Resolve a project by name from the global config (owned clone so it can
/// move into a blocking task).
fn project_by_name(name: &str) -> Result<void_stack_core::model::Project, String> {
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
}
