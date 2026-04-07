use void_stack_core::global_config::load_global_config;

use crate::state::AppState;

#[tauri::command]
pub fn index_project_codebase_cmd(project_name: String, force: bool) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project_name)?;
    let stats = void_stack_core::vector_index::index_project(&proj, force, |_, _| {})
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&stats).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn semantic_search_cmd(
    project_name: String,
    query: String,
    top_k: Option<usize>,
) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project_name)?;
    let results = void_stack_core::vector_index::semantic_search(&proj, &query, top_k.unwrap_or(5))
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_index_stats_cmd(project_name: String) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project_name)?;
    match void_stack_core::vector_index::get_index_stats(&proj) {
        Ok(Some(stats)) => serde_json::to_string_pretty(&stats).map_err(|e| e.to_string()),
        Ok(None) => Ok("null".to_string()),
        Err(e) => Err(e),
    }
}
