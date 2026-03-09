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
    /// Path to saved .drawio file (only when format is drawio)
    pub saved_path: Option<String>,
}

#[tauri::command]
pub fn generate_diagram(project: String, format: Option<String>) -> Result<DiagramResult, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let fmt = format.as_deref().unwrap_or("drawio");

    if fmt == "drawio" {
        // Generate Draw.io XML per section for in-app rendering
        let arch_xml = diagram::drawio::generate_architecture(&proj);
        let api_xml = diagram::drawio::generate_api_routes(&proj);
        let db_xml = diagram::drawio::generate_db_models(&proj);

        // Also save the combined multi-page .drawio file
        let drawio_xml = diagram::drawio::generate_all(&proj);
        let clean_path = strip_win_prefix(&proj.path);
        let file_path = std::path::Path::new(&clean_path).join("void-stack-diagrams.drawio");
        let saved_path = match std::fs::write(&file_path, &drawio_xml) {
            Ok(_) => Some(file_path.to_string_lossy().to_string()),
            Err(e) => {
                eprintln!("Failed to save .drawio: {}", e);
                None
            }
        };

        Ok(DiagramResult {
            architecture: arch_xml,
            api_routes: api_xml,
            db_models: db_xml,
            warnings: Vec::new(),
            format: fmt.to_string(),
            saved_path,
        })
    } else {
        // Mermaid format
        let diagrams = diagram::generate_all(&proj);

        Ok(DiagramResult {
            architecture: diagrams.architecture,
            api_routes: diagrams.api_routes,
            db_models: diagrams.db_models,
            warnings: diagrams.warnings,
            format: fmt.to_string(),
            saved_path: None,
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
