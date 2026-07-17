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

// scan_global_space is deliberately untested: scan_global() reads the real
// user home / local-app caches (npm, pip, cargo, Ollama models). It is not
// isolatable via env and its results are machine-dependent, so a unit test
// would be non-deterministic. Covered indirectly by void-stack-core tests.

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

#[cfg(test)]
mod tests {
    use super::*;
    use void_stack_core::model::Project;

    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    fn project_at(path: &str) -> Project {
        Project {
            name: "space-fixture".to_string(),
            description: String::new(),
            path: path.to_string(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    /// Create `<root>/<dir>` holding a single file over the scanner's 500KB
    /// threshold so the directory is reported.
    fn heavy_dir(root: &std::path::Path, dir: &str) {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("blob.bin"), vec![0u8; 600_000]).unwrap();
    }

    #[tokio::test]
    async fn test_scan_project_space_reports_heavy_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        heavy_dir(tmp.path(), "node_modules");
        heavy_dir(tmp.path(), "target");
        let project = project_at(&tmp.path().to_string_lossy());

        let out = text_of(&scan_project_space(&project).await.unwrap());
        assert!(out.contains("Disk space scan"), "got: {out}");
        assert!(out.contains("node_modules"), "got: {out}");
        assert!(out.contains("target"), "got: {out}");
        assert!(out.contains("safe to delete"), "got: {out}");
    }

    #[tokio::test]
    async fn test_scan_project_space_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_at(&tmp.path().to_string_lossy());

        let out = text_of(&scan_project_space(&project).await.unwrap());
        assert!(out.contains("No heavy directories found"), "got: {out}");
    }

    /// Small directories (under the 500KB threshold) are ignored.
    #[tokio::test]
    async fn test_scan_project_space_below_threshold_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let nm = tmp.path().join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("tiny.txt"), b"small").unwrap();
        let project = project_at(&tmp.path().to_string_lossy());

        let out = text_of(&scan_project_space(&project).await.unwrap());
        assert!(out.contains("No heavy directories found"), "got: {out}");
    }
}
