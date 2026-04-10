use serde::Serialize;
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

#[derive(Serialize)]
pub struct FilteredLogsResult {
    pub lines: Vec<String>,
    pub lines_original: usize,
    pub lines_filtered: usize,
    pub savings_pct: f32,
}

#[tauri::command]
pub fn filter_logs_cmd(raw_lines: Vec<String>, compact: bool) -> FilteredLogsResult {
    let filtered = void_stack_core::log_filter::filter_log_lines(&raw_lines, compact);
    let lines_original = raw_lines.len();
    let lines_filtered = filtered.len();
    let savings_pct = if lines_original > 0 {
        ((1.0 - (lines_filtered as f32 / lines_original as f32)) * 100.0).max(0.0)
    } else {
        0.0
    };
    FilteredLogsResult {
        lines: filtered,
        lines_original,
        lines_filtered,
        savings_pct,
    }
}
