use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

use super::format_size;

/// Logic for scan_project_space tool.
pub async fn scan_project_space(project: &Project) -> Result<CallToolResult, McpError> {
    let path = strip_win_prefix(&project.path);
    let proj_name = project.name.clone();

    let entries = tokio::task::spawn_blocking(move || {
        void_stack_core::space::scan_project(std::path::Path::new(&path))
    })
    .await
    .map_err(|e| McpError::internal_error(format!("Scan failed: {}", e), None))?;

    if entries.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No heavy directories found in '{}'.",
            proj_name,
        ))]));
    }

    let total: u64 = entries.iter().map(|e| e.size_bytes).sum();
    let total_human = format_size(total);

    let mut lines = vec![format!(
        "Disk space scan for '{}' — {} reclaimable ({}):\n",
        proj_name,
        total_human,
        entries.len(),
    )];
    for entry in &entries {
        let deletable = if entry.deletable {
            "safe to delete"
        } else {
            "NOT safe"
        };
        lines.push(format!(
            "  - {} [{}] — {} ({})\n    path: {}\n    restore: {}",
            entry.name, entry.category, entry.size_human, deletable, entry.path, entry.restore_hint,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(
        lines.join("\n"),
    )]))
}

/// Logic for scan_global_space tool.
pub async fn scan_global_space() -> Result<CallToolResult, McpError> {
    let entries = tokio::task::spawn_blocking(void_stack_core::space::scan_global)
        .await
        .map_err(|e| McpError::internal_error(format!("Scan failed: {}", e), None))?;

    if entries.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No global caches or model storage found.".to_string(),
        )]));
    }

    let total: u64 = entries.iter().map(|e| e.size_bytes).sum();
    let total_human = format_size(total);

    let mut lines = vec![format!(
        "Global disk space scan — {} total ({} entries):\n",
        total_human,
        entries.len(),
    )];
    for entry in &entries {
        let deletable = if entry.deletable {
            "safe to delete"
        } else {
            "NOT safe"
        };
        lines.push(format!(
            "  - {} [{}] — {} ({})\n    path: {}\n    restore: {}",
            entry.name, entry.category, entry.size_human, deletable, entry.path, entry.restore_hint,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(
        lines.join("\n"),
    )]))
}
