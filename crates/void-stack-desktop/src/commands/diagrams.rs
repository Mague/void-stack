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

    // One scanner pass; both renderers consume the same IR, so the formats
    // (and their warnings) can no longer drift apart.
    let ir = diagram::ir::build_ir(&proj);

    if fmt == "drawio" {
        let pages = diagram::drawio::render_all_from_ir(&ir);

        // Save the combined multi-page .drawio file
        let clean_path = strip_win_prefix(&proj.path);
        let file_path = std::path::Path::new(&clean_path).join("void-stack-diagrams.drawio");
        let saved_path = match std::fs::write(&file_path, &pages.combined) {
            Ok(_) => Some(file_path.to_string_lossy().to_string()),
            Err(e) => {
                eprintln!("Failed to save .drawio: {}", e);
                None
            }
        };

        Ok(DiagramResult {
            architecture: pages.architecture,
            api_routes: pages.api_routes,
            db_models: pages.db_models,
            warnings: pages.warnings,
            format: fmt.to_string(),
            saved_path,
        })
    } else {
        let mermaid = diagram::generate_all_from_ir(&ir);
        Ok(DiagramResult {
            architecture: mermaid.architecture,
            api_routes: mermaid.api_routes,
            db_models: mermaid.db_models,
            warnings: mermaid.warnings,
            format: fmt.to_string(),
            saved_path: None,
        })
    }
}

#[tauri::command]
pub fn generate_graph_html(project: String) -> Result<String, String> {
    use void_stack_core::structural::graph;

    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    // The interactive graph.html is built from the structural call graph. If
    // it was never built, fail with a detectable, actionable message instead
    // of a cryptic SQLite/IO error. Don't open_db unless the file exists, so
    // we never create an empty DB just to check.
    let has_graph = graph::structural_db_path(&proj).exists()
        && graph::open_db(&proj)
            .and_then(|conn| graph::count_nodes(&conn))
            .map(|n| n > 0)
            .unwrap_or(false);
    if !has_graph {
        return Err("GRAPH_NOT_BUILT: El grafo estructural no existe para este \
             proyecto. Construye el grafo antes de usar Graph HTML."
            .to_string());
    }

    let path = diagram::graph_html::generate_graph_html(&proj)?;
    Ok(path.to_string_lossy().to_string())
}

/// Return the interactive graph.html content as a string, for the in-app
/// viewer (rendered inside an iframe). Built from the dependency/import
/// graph — no structural graph required. Errors clearly if no graph can be
/// built (e.g. no recognizable source files).
#[tauri::command]
pub fn get_graph_html_cmd(project: String) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    diagram::graph_html::build_graph_html(&proj)
}

#[tauri::command]
pub fn save_diagram_file(
    project: String,
    content: String,
    extension: String,
) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let clean_path = strip_win_prefix(&proj.path);
    let filename = format!("void-stack-diagrams.{}", extension);
    let file_path = std::path::Path::new(&clean_path).join(&filename);

    std::fs::write(&file_path, &content)
        .map_err(|e| format!("Failed to save {}: {}", file_path.display(), e))?;

    Ok(file_path.to_string_lossy().to_string())
}
