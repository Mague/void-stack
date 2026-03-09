pub mod projects;
pub mod services;
pub mod analysis;
pub mod diagrams;
pub mod docker;
pub mod docs;
pub mod debt;
pub mod space;
pub mod suggest;

use rmcp::ErrorData as McpError;

/// Format bytes into human-readable size.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// List documentation files in a project directory.
pub fn list_doc_files(root: &str) -> Vec<String> {
    let path = std::path::Path::new(root);
    let doc_extensions = ["md", "txt"];
    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(ext) = std::path::Path::new(&name).extension().and_then(|e| e.to_str()) {
                if doc_extensions.contains(&ext) {
                    files.push(format!("  - {}", name));
                }
            }
        }
    }
    files.sort();
    files
}

/// Helper to serialize to pretty JSON or return an MCP internal error.
pub fn to_json_pretty<T: serde::Serialize>(value: &T) -> Result<String, McpError> {
    serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(e.to_string(), None))
}
