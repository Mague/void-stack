//! Board commands: kanban board stored as BOARD.md in the managed repo.

use std::path::PathBuf;

use void_stack_core::board;
use void_stack_core::global_config::load_global_config;
use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

fn resolve(project_name: &str) -> Result<(Project, PathBuf), String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, project_name)?;
    let root = PathBuf::from(strip_win_prefix(&proj.path));
    Ok((proj, root))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Run a git-history walk off the UI thread. The history commands shell
/// out to git; synchronous Tauri commands would block the main thread for
/// the whole walk and freeze the window.
async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("task join error: {}", e))?
}

#[tauri::command]
pub fn board_get_cmd(project: String) -> Result<board::Board, String> {
    let (proj, root) = resolve(&project)?;
    board::load_board(&root, &proj.name)
}

#[tauri::command]
pub fn board_add_task_cmd(
    project: String,
    title: String,
    priority: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<board::Board, String> {
    let (proj, root) = resolve(&project)?;
    let mut b = board::load_board(&root, &proj.name)?;
    board::add_task(
        &mut b,
        &title,
        priority.as_deref(),
        &tags.unwrap_or_default(),
        &today(),
    );
    board::save_board(&root, &b)?;
    Ok(b)
}

#[tauri::command]
pub fn board_move_task_cmd(
    project: String,
    id: String,
    column: String,
) -> Result<board::Board, String> {
    let (proj, root) = resolve(&project)?;
    let mut b = board::load_board(&root, &proj.name)?;
    board::move_task(&mut b, &id, &column)?;
    board::save_board(&root, &b)?;
    Ok(b)
}

#[tauri::command]
pub fn board_edit_task_cmd(
    project: String,
    id: String,
    title: Option<String>,
    priority: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<board::Board, String> {
    let (proj, root) = resolve(&project)?;
    let mut b = board::load_board(&root, &proj.name)?;
    board::edit_task(
        &mut b,
        &id,
        title.as_deref(),
        priority.as_deref(),
        tags.as_deref(),
    )?;
    board::save_board(&root, &b)?;
    Ok(b)
}

#[tauri::command]
pub async fn board_history_cmd(
    project: String,
) -> Result<Vec<void_stack_core::boardhistory::TaskHistory>, String> {
    let (proj, root) = resolve(&project)?;
    blocking(move || void_stack_core::boardhistory::board_history(&root, &proj.name)).await
}

#[tauri::command]
pub async fn board_task_history_cmd(
    project: String,
    id: String,
) -> Result<void_stack_core::boardhistory::TaskHistory, String> {
    let (proj, root) = resolve(&project)?;
    blocking(move || void_stack_core::boardhistory::task_history(&root, &proj.name, &id)).await
}

#[tauri::command]
pub async fn board_timeline_cmd(
    project: String,
    by: String,
    since: Option<String>,
) -> Result<Vec<void_stack_core::timeline::TimelineBucket>, String> {
    let (proj, root) = resolve(&project)?;
    let group = void_stack_core::timeline::GroupBy::parse(&by)?;
    blocking(move || {
        void_stack_core::timeline::board_timeline(&root, &proj.name, group, since.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn board_commit_detail_cmd(
    project: String,
    hash: String,
) -> Result<void_stack_core::timeline::CommitDetail, String> {
    let (_proj, root) = resolve(&project)?;
    blocking(move || void_stack_core::timeline::commit_detail(&root, &hash)).await
}

#[tauri::command]
pub fn board_archive_cmd(project: String, days: Option<i64>) -> Result<board::Board, String> {
    let (proj, root) = resolve(&project)?;
    let mut b = board::load_board(&root, &proj.name)?;
    board::archive_done(
        &root,
        &mut b,
        days.unwrap_or(14),
        chrono::Local::now().date_naive(),
    )?;
    board::save_board(&root, &b)?;
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support;

    #[test]
    fn test_today_format() {
        let t = today();
        // YYYY-MM-DD → 10 chars, two dashes.
        assert_eq!(t.len(), 10);
        assert_eq!(t.matches('-').count(), 2);
    }

    #[test]
    fn test_board_get_unknown_project_errors() {
        let _g = test_support::config_guard();
        assert!(board_get_cmd("Ghost".to_string()).is_err());
    }

    #[test]
    fn test_board_get_empty_returns_default_columns() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("B", dir.path()));

        let board = board_get_cmd("B".to_string()).unwrap();
        // Default board has the four standard columns and no tasks yet.
        assert_eq!(board.columns.len(), 4);
        assert!(board.columns.iter().all(|c| c.tasks.is_empty()));
    }

    #[test]
    fn test_board_add_move_edit_flow() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Flow", dir.path()));

        // Add a task → lands in Backlog.
        let board = board_add_task_cmd(
            "Flow".to_string(),
            "Fix login".to_string(),
            Some("high".to_string()),
            Some(vec!["auth".to_string()]),
        )
        .unwrap();
        let backlog = board
            .columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case("Backlog"))
            .unwrap();
        assert_eq!(backlog.tasks.len(), 1);
        let id = backlog.tasks[0].id.clone();
        assert_eq!(backlog.tasks[0].title, "Fix login");

        // Move it to Done.
        let moved =
            board_move_task_cmd("Flow".to_string(), id.clone(), "Done".to_string()).unwrap();
        let done = moved
            .columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case("Done"))
            .unwrap();
        assert_eq!(done.tasks.len(), 1);

        // Moving to an unknown column errors.
        assert!(
            board_move_task_cmd("Flow".to_string(), id.clone(), "Nowhere".to_string()).is_err()
        );

        // Edit the title.
        let edited = board_edit_task_cmd(
            "Flow".to_string(),
            id.clone(),
            Some("Fix login v2".to_string()),
            None,
            None,
        )
        .unwrap();
        let title = edited
            .columns
            .iter()
            .flat_map(|c| &c.tasks)
            .find(|t| t.id == id)
            .map(|t| t.title.clone())
            .unwrap();
        assert_eq!(title, "Fix login v2");
    }

    #[test]
    fn test_board_archive_returns_board() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Arch", dir.path()));

        board_add_task_cmd("Arch".to_string(), "Task".to_string(), None, None).unwrap();
        // Archiving with a large window keeps everything; just must not error.
        let board = board_archive_cmd("Arch".to_string(), Some(365)).unwrap();
        assert_eq!(board.columns.len(), 4);
    }
}
