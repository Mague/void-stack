#[tauri::command]
pub fn get_token_stats_cmd(project: Option<String>, days: Option<u32>) -> Result<String, String> {
    let report = void_stack_core::stats::get_stats(project.as_deref(), days.unwrap_or(30))
        .map_err(|e| format!("Error loading stats: {}", e))?;

    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
}
