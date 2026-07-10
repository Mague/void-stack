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
pub fn board_history_cmd(
    project: String,
) -> Result<Vec<void_stack_core::boardhistory::TaskHistory>, String> {
    let (proj, root) = resolve(&project)?;
    void_stack_core::boardhistory::board_history(&root, &proj.name)
}

#[tauri::command]
pub fn board_task_history_cmd(
    project: String,
    id: String,
) -> Result<void_stack_core::boardhistory::TaskHistory, String> {
    let (proj, root) = resolve(&project)?;
    void_stack_core::boardhistory::task_history(&root, &proj.name, &id)
}

#[tauri::command]
pub fn board_timeline_cmd(
    project: String,
    by: String,
    since: Option<String>,
) -> Result<Vec<void_stack_core::timeline::TimelineBucket>, String> {
    let (proj, root) = resolve(&project)?;
    let group = void_stack_core::timeline::GroupBy::parse(&by)?;
    void_stack_core::timeline::board_timeline(&root, &proj.name, group, since.as_deref())
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
