use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

/// Logic for save_debt_snapshot tool.
pub fn save_debt_snapshot(
    project: &Project,
    label: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let mut analysis_results: Vec<(String, void_stack_core::analyzer::AnalysisResult)> =
        Vec::new();

    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let clean = strip_win_prefix(dir);
        let path = std::path::Path::new(&clean);
        if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
            analysis_results.push((svc.name.clone(), result));
        }
    }

    if analysis_results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No analyzable code found in any service. Snapshot not created.".to_string(),
        )]));
    }

    let snapshot = void_stack_core::analyzer::history::create_snapshot(
        &analysis_results,
        label.map(|s| s.to_string()),
    );

    let root = strip_win_prefix(&project.path);
    let root_path = std::path::Path::new(&root);
    void_stack_core::analyzer::history::save_snapshot(root_path, &snapshot)
        .map_err(|e| McpError::internal_error(format!("Failed to save snapshot: {}", e), None))?;

    let label_str = label.unwrap_or("(unlabeled)");
    let svc_count = snapshot.services.len();
    let total_loc: usize = snapshot.services.iter().map(|s| s.total_loc).sum();
    let total_ap: usize = snapshot.services.iter().map(|s| s.anti_pattern_count).sum();

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Snapshot saved for '{}' (label: {})\n  Services analyzed: {}\n  Total LOC: {}\n  Total anti-patterns: {}\n  Timestamp: {}",
        project.name,
        label_str,
        svc_count,
        total_loc,
        total_ap,
        snapshot.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
    ))]))
}

/// Logic for list_debt_snapshots tool.
pub fn list_debt_snapshots(project: &Project) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let snapshots =
        void_stack_core::analyzer::history::load_snapshots(std::path::Path::new(&root));

    if snapshots.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No debt snapshots found for '{}'. Use save_debt_snapshot to create one.",
            project.name,
        ))]));
    }

    let mut lines = vec![format!(
        "Debt snapshots for '{}' ({} total):\n",
        project.name,
        snapshots.len()
    )];
    for (i, snap) in snapshots.iter().enumerate() {
        let label = snap.label.as_deref().unwrap_or("-");
        let total_loc: usize = snap.services.iter().map(|s| s.total_loc).sum();
        let total_ap: usize = snap.services.iter().map(|s| s.anti_pattern_count).sum();
        lines.push(format!(
            "  [{}] {} | label: {} | services: {} | LOC: {} | anti-patterns: {}",
            i,
            snap.timestamp.format("%Y-%m-%d %H:%M"),
            label,
            snap.services.len(),
            total_loc,
            total_ap,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(
        lines.join("\n"),
    )]))
}

/// Logic for compare_debt tool.
pub fn compare_debt(
    project: &Project,
    index_a: Option<usize>,
    index_b: Option<usize>,
) -> Result<CallToolResult, McpError> {
    let root = strip_win_prefix(&project.path);
    let snapshots =
        void_stack_core::analyzer::history::load_snapshots(std::path::Path::new(&root));

    if snapshots.len() < 2 {
        return Err(McpError::invalid_params(
            format!(
                "Need at least 2 snapshots to compare. Project '{}' has {}.",
                project.name,
                snapshots.len(),
            ),
            None,
        ));
    }

    let idx_a = index_a.unwrap_or(snapshots.len() - 2);
    let idx_b = index_b.unwrap_or(snapshots.len() - 1);

    if idx_a >= snapshots.len() || idx_b >= snapshots.len() {
        return Err(McpError::invalid_params(
            format!(
                "Index out of range. Valid range: 0..{} (total: {} snapshots)",
                snapshots.len() - 1,
                snapshots.len(),
            ),
            None,
        ));
    }

    let comparison =
        void_stack_core::analyzer::history::compare(&snapshots[idx_a], &snapshots[idx_b]);
    let markdown = void_stack_core::analyzer::history::comparison_markdown(&comparison);

    Ok(CallToolResult::success(vec![Content::text(markdown)]))
}
