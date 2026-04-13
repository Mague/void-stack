//! MCP tools backed by the structural graph.

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use super::to_json_pretty;
use void_stack_core::model::Project;

/// Build (or incrementally update) the structural call graph for a project.
pub fn build_structural_graph_tool(
    project: &Project,
    force: bool,
) -> Result<CallToolResult, McpError> {
    let stats = void_stack_core::structural::build_structural_graph(project, force)
        .map_err(|e| McpError::internal_error(e, None))?;
    let json = to_json_pretty(&stats)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Compute the bidirectional blast radius for a set of changed files.
/// When `changed_files` is None, auto-detect via `git diff HEAD~1`.
pub fn get_impact_radius_tool(
    project: &Project,
    changed_files: Option<Vec<String>>,
    max_depth: Option<usize>,
) -> Result<CallToolResult, McpError> {
    let files = match changed_files {
        Some(f) if !f.is_empty() => f,
        _ => {
            let root = std::path::Path::new(&project.path);
            void_stack_core::vector_index::stats::get_git_changed_files(root, "HEAD~1")
        }
    };

    if files.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No changed files to analyze. Pass changed_files or make sure \
             the project is a git repo with commits against HEAD~1."
                .to_string(),
        )]));
    }

    let conn = void_stack_core::structural::open_db(project)
        .map_err(|e| McpError::internal_error(e, None))?;
    let res =
        void_stack_core::structural::get_impact_radius(&conn, &files, max_depth.unwrap_or(2), 500)
            .map_err(|e| McpError::internal_error(e, None))?;
    let json = to_json_pretty(&res)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Query the structural graph: callers, callees, tests, or fuzzy search.
pub fn query_graph_tool(
    project: &Project,
    target: String,
    query_type: String,
) -> Result<CallToolResult, McpError> {
    let conn = void_stack_core::structural::open_db(project)
        .map_err(|e| McpError::internal_error(e, None))?;

    let nodes = match query_type.as_str() {
        "callers" => void_stack_core::structural::get_callers(&conn, &target),
        "callees" => void_stack_core::structural::get_callees(&conn, &target),
        "tests" => void_stack_core::structural::get_tests_for(&conn, &target),
        "search" => void_stack_core::structural::search_nodes(&conn, &target, 50),
        other => {
            return Err(McpError::invalid_params(
                format!(
                    "Unknown query_type '{}'. Expected one of: callers, callees, tests, search.",
                    other
                ),
                None,
            ));
        }
    };

    let json = to_json_pretty(&nodes)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}
