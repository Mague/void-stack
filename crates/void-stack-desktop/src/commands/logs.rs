use tauri::State;

use void_stack_core::global_config::load_global_config;

use crate::state::AppState;

#[tauri::command]
pub async fn get_logs(
    project: String,
    service: String,
    lines: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let mgr = state.get_manager(&proj).await;
    let all_logs = mgr.get_logs(&service).await;
    let n = lines.unwrap_or(100);
    let start = if all_logs.len() > n {
        all_logs.len() - n
    } else {
        0
    };
    Ok(all_logs[start..].to_vec())
}
