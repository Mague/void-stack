use serde::Serialize;

use devlaunch_core::diagram;
use devlaunch_core::global_config::load_global_config;

use crate::state::AppState;

#[derive(Serialize)]
pub struct DiagramResult {
    pub architecture: String,
    pub api_routes: Option<String>,
    pub db_models: Option<String>,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub fn generate_diagram(project: String) -> Result<DiagramResult, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let diagrams = diagram::generate_all(&proj);
    Ok(DiagramResult {
        architecture: diagrams.architecture,
        api_routes: diagrams.api_routes,
        db_models: diagrams.db_models,
        warnings: diagrams.warnings,
    })
}
