use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

use super::list_doc_files;

/// Logic for read_project_docs tool.
pub fn read_project_docs(project: &Project, filename: &str) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let doc_path = std::path::Path::new(&root).join(filename);

    // Security: only allow reading markdown/text files within the project
    let allowed_extensions = ["md", "txt", "toml", "json", "yml", "yaml"];
    let ext = doc_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !allowed_extensions.contains(&ext) {
        return Err(McpError::invalid_params(
            format!(
                "File type '.{}' not allowed. Use: {}",
                ext,
                allowed_extensions.join(", ")
            ),
            None,
        ));
    }
    // Block sensitive files (secrets, credentials, .env)
    if void_stack_core::security::is_sensitive_file(&doc_path) {
        return Err(McpError::invalid_params(
            "Cannot read sensitive/credential files".to_string(),
            None,
        ));
    }

    match std::fs::read_to_string(&doc_path) {
        Ok(content) => {
            // Truncate very large files
            let truncated = if content.len() > 50_000 {
                format!(
                    "{}...\n\n[truncated, {} bytes total]",
                    &content[..50_000],
                    content.len()
                )
            } else {
                content
            };
            Ok(CallToolResult::success(vec![Content::text(truncated)]))
        }
        Err(_) => {
            // List available doc files
            let available = list_doc_files(&root);
            let msg = if available.is_empty() {
                format!(
                    "'{}' not found in '{}'. No documentation files found.",
                    filename, project.name
                )
            } else {
                format!(
                    "'{}' not found in '{}'. Available files:\n{}",
                    filename,
                    project.name,
                    available.join("\n")
                )
            };
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        }
    }
}

/// Logic for read_all_docs tool.
pub fn read_all_docs(project: &Project) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let doc_extensions = ["md", "txt"];
    let mut docs = Vec::new();
    let mut total_size = 0usize;
    let max_total = 100_000; // 100KB total limit

    // Scan root directory for doc files
    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut files: Vec<_> = entries
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let ext = std::path::Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                doc_extensions.contains(&ext)
            })
            .collect();
        files.sort_by_key(|e| e.file_name());

        // Prioritize important files first
        let priority = ["README.md", "CLAUDE.md", "CHANGELOG.md", "CONTRIBUTING.md"];
        files.sort_by_key(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let idx = priority.iter().position(|p| p.eq_ignore_ascii_case(&name));
            idx.unwrap_or(priority.len())
        });

        for entry in files {
            if total_size >= max_total {
                docs.push(format!(
                    "\n---\n[Truncated: reached {}KB limit]\n",
                    max_total / 1000
                ));
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if let Ok(content) = std::fs::read_to_string(&path) {
                let remaining = max_total - total_size;
                let truncated = if content.len() > remaining {
                    format!(
                        "{}...\n[truncated, {} bytes total]",
                        &content[..remaining],
                        content.len()
                    )
                } else {
                    content.clone()
                };
                total_size += truncated.len();
                docs.push(format!("# === {} ===\n\n{}\n", name, truncated));
            }
        }
    }

    // Also check for void-stack-analysis.md
    let analysis_path = std::path::Path::new(&root).join("void-stack-analysis.md");
    if analysis_path.exists()
        && total_size < max_total
        && let Ok(content) = std::fs::read_to_string(&analysis_path)
    {
        let remaining = max_total - total_size;
        let truncated = if content.len() > remaining {
            format!("{}...\n[truncated]", &content[..remaining])
        } else {
            content
        };
        docs.push(format!(
            "# === void-stack-analysis.md ===\n\n{}\n",
            truncated
        ));
    }

    if docs.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No documentation files found in '{}'.",
            project.name
        ))]));
    }

    Ok(CallToolResult::success(vec![Content::text(
        docs.join("\n---\n\n"),
    )]))
}
