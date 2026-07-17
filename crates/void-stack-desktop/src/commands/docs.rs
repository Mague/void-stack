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

/// Generate a .claudeignore file for a project based on its detected tech stack.
#[tauri::command]
pub fn generate_claudeignore_cmd(
    project_name: String,
    dry_run: Option<bool>,
) -> Result<String, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project_name)?;
    let base = strip_win_prefix(&proj.path);
    let project_path = std::path::Path::new(&base);

    let result = void_stack_core::claudeignore::generate_claudeignore(project_path);

    let mut output = result.content.clone();
    output.push_str(&format!(
        "\n---\n{} patterns | ~{} files ignored",
        result.patterns_count, result.estimated_files_ignored
    ));

    if !dry_run.unwrap_or(false) {
        match void_stack_core::claudeignore::save_claudeignore(project_path, &result.content) {
            Ok(path) => {
                output.push_str(&format!("\n✓ Saved to {}", path.display()));
            }
            Err(e) => {
                return Err(format!("Error saving .claudeignore: {}", e));
            }
        }
    }

    Ok(output)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support;

    #[test]
    fn test_read_project_readme_found_and_missing() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Doc", dir.path()));

        // No README yet.
        assert!(read_project_readme("Doc".to_string()).is_err());

        std::fs::write(dir.path().join("README.md"), "# Hello").unwrap();
        let content = read_project_readme("Doc".to_string()).unwrap();
        assert!(content.contains("Hello"));
    }

    #[test]
    fn test_read_project_readme_unknown_project_errors() {
        let _g = test_support::config_guard();
        assert!(read_project_readme("Ghost".to_string()).is_err());
    }

    #[test]
    fn test_list_project_docs_filters_and_sorts() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zeta.md"), "z").unwrap();
        std::fs::write(dir.path().join("alpha.txt"), "a").unwrap();
        std::fs::write(dir.path().join("code.rs"), "fn x() {}").unwrap();
        test_support::register(test_support::project("Docs", dir.path()));

        let docs = list_project_docs("Docs".to_string()).unwrap();
        assert_eq!(docs, vec!["alpha.txt", "zeta.md"]);
    }

    #[test]
    fn test_read_project_doc_traversal_rejected() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Trav", dir.path()));

        // Path traversal / separators are rejected before touching the FS.
        assert!(read_project_doc("Trav".to_string(), "../secret.md".to_string()).is_err());
        assert!(read_project_doc("Trav".to_string(), "sub/file.md".to_string()).is_err());
        assert!(read_project_doc("Trav".to_string(), "sub\\file.md".to_string()).is_err());
    }

    #[test]
    fn test_read_project_doc_missing_and_valid() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Rd", dir.path()));

        assert!(read_project_doc("Rd".to_string(), "NOPE.md".to_string()).is_err());

        std::fs::write(dir.path().join("NOTES.md"), "notes body").unwrap();
        let body = read_project_doc("Rd".to_string(), "NOTES.md".to_string()).unwrap();
        assert_eq!(body, "notes body");
    }

    #[test]
    fn test_read_and_list_project_files() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        test_support::register(test_support::project("Files", dir.path()));

        let listed = list_project_files_cmd("Files".to_string()).unwrap();
        assert!(listed.iter().any(|f| f.contains("main.rs")));

        let content = read_project_file_cmd("Files".to_string(), "main.rs".to_string()).unwrap();
        assert!(content.contains("fn main"));
    }

    #[test]
    fn test_generate_claudeignore_dry_run_does_not_save() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        test_support::register(test_support::project("Ci", dir.path()));

        let out = generate_claudeignore_cmd("Ci".to_string(), Some(true)).unwrap();
        assert!(out.contains("patterns"));
        // Dry run must not write the file.
        assert!(!dir.path().join(".claudeignore").exists());
    }
}
