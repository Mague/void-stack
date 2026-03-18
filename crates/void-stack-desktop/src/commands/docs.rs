use void_stack_core::file_reader;
use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;
use void_stack_core::security;

use crate::state::AppState;

/// Read a doc file (README, CHANGELOG, etc.) from a project directory.
#[tauri::command]
pub fn read_project_readme(project: String) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let base = strip_win_prefix(&proj.path);
    let base_path = std::path::Path::new(&base);

    // Try common README filenames
    let candidates = [
        "README.md",
        "readme.md",
        "Readme.md",
        "README.MD",
        "README",
        "README.txt",
        "README.rst",
    ];

    for name in &candidates {
        let doc_path = base_path.join(name);
        if doc_path.exists() {
            if security::is_sensitive_file(&doc_path) {
                return Err("Archivo bloqueado por seguridad".to_string());
            }
            let content = std::fs::read_to_string(&doc_path)
                .map_err(|e| format!("Error leyendo {}: {}", name, e))?;
            return Ok(content);
        }
    }

    Err("No se encontró README en el proyecto".to_string())
}

/// List available doc files in a project.
#[tauri::command]
pub fn list_project_docs(project: String) -> Result<Vec<String>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let base = strip_win_prefix(&proj.path);
    let base_path = std::path::Path::new(&base);

    let doc_extensions = ["md", "txt", "rst", "adoc"];
    let mut docs = Vec::new();

    if let Ok(entries) = std::fs::read_dir(base_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if security::is_sensitive_file(&path) {
                continue;
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str())
                && doc_extensions.contains(&ext.to_lowercase().as_str())
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
            {
                docs.push(name.to_string());
            }
        }
    }

    docs.sort();
    Ok(docs)
}

/// Read a specific doc file by name from the project root.
#[tauri::command]
pub fn read_project_doc(project: String, filename: String) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let base = strip_win_prefix(&proj.path);
    let doc_path = std::path::Path::new(&base).join(&filename);

    // Security: block path traversal
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err("Ruta no permitida".to_string());
    }

    if security::is_sensitive_file(&doc_path) {
        return Err("Archivo bloqueado por seguridad".to_string());
    }

    if !doc_path.exists() {
        return Err(format!("Archivo '{}' no encontrado", filename));
    }

    std::fs::read_to_string(&doc_path).map_err(|e| format!("Error leyendo {}: {}", filename, e))
}

/// Read any file from a project by relative path (respects security).
#[tauri::command]
pub fn read_project_file_cmd(project: String, path: String) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let base = strip_win_prefix(&proj.path);
    let project_path = std::path::Path::new(&base);
    file_reader::read_project_file(project_path, &path).map_err(|e| e.to_string())
}

/// List all files in a project (up to 3 levels deep, excludes sensitive).
#[tauri::command]
pub fn list_project_files_cmd(project: String) -> Result<Vec<String>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let base = strip_win_prefix(&proj.path);
    let project_path = std::path::Path::new(&base);
    Ok(file_reader::list_project_files(project_path))
}
