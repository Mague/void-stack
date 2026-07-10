//! Daily briefing command: consolidated report for the active projects.

use void_stack_core::briefing;
use void_stack_core::global_config::load_global_config;

/// Markdown briefing for the active projects, or for `only` when given
/// (e.g. just the currently selected project). Runs audits per project,
/// so it executes on a blocking thread.
#[tauri::command]
pub async fn daily_briefing_cmd(only: Option<Vec<String>>) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || briefing::generate_briefing(&config, only.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Projects currently marked active for the briefing.
#[tauri::command]
pub fn briefing_active_cmd() -> Result<Vec<String>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    Ok(config.briefing.active_projects.clone())
}
