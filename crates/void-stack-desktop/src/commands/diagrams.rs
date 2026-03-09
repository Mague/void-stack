use serde::Serialize;

use void_stack_core::diagram;
use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct DiagramResult {
    pub architecture: String,
    pub api_routes: Option<String>,
    pub db_models: Option<String>,
    pub warnings: Vec<String>,
    pub format: String,
}

#[tauri::command]
pub fn generate_diagram(project: String, format: Option<String>) -> Result<DiagramResult, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let fmt = format.as_deref().unwrap_or("mermaid");

    if fmt == "drawio" {
        let drawio_xml = diagram::drawio::generate_all(&proj);
        Ok(DiagramResult {
            architecture: drawio_xml,
            api_routes: None,
            db_models: None,
            warnings: vec![],
            format: "drawio".to_string(),
        })
    } else {
        let diagrams = diagram::generate_all(&proj);
        Ok(DiagramResult {
            architecture: diagrams.architecture,
            api_routes: diagrams.api_routes,
            db_models: diagrams.db_models,
            warnings: diagrams.warnings,
            format: "mermaid".to_string(),
        })
    }
}

#[tauri::command]
pub fn save_diagram_file(project: String, content: String, extension: String) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let clean_path = strip_win_prefix(&proj.path);
    let filename = format!("void-stack-diagrams.{}", extension);
    let file_path = std::path::Path::new(&clean_path).join(&filename);

    std::fs::write(&file_path, &content)
        .map_err(|e| format!("Failed to save {}: {}", file_path.display(), e))?;

    Ok(file_path.to_string_lossy().to_string())
}
