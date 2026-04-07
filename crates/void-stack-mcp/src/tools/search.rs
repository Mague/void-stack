use rmcp::ErrorData as McpError;
use rmcp::model::*;

use super::to_json_pretty;
use void_stack_core::model::Project;

/// Logic for index_project_codebase tool.
pub fn index_project_codebase(project: &Project, force: bool) -> Result<CallToolResult, McpError> {
    let stats = void_stack_core::vector_index::index_project(project, force, |_, _| {})
        .map_err(|e| McpError::internal_error(format!("Indexing failed: {}", e), None))?;

    let json = to_json_pretty(&stats)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Logic for semantic_search tool.
pub fn semantic_search(
    project: &Project,
    query: &str,
    top_k: usize,
) -> Result<CallToolResult, McpError> {
    let results = void_stack_core::vector_index::semantic_search(project, query, top_k)
        .map_err(|e| McpError::internal_error(format!("Search failed: {}", e), None))?;

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No results found for: \"{}\"",
            query
        ))]));
    }

    // Format results as structured text for LLM consumption
    let mut output = String::new();
    for (i, r) in results.iter().enumerate() {
        output.push_str(&format!(
            "## {}. {} (score: {:.2}, lines {}-{})\n\n```\n{}\n```\n\n",
            i + 1,
            r.file_path,
            r.score,
            r.line_start,
            r.line_end,
            r.chunk
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

/// Logic for get_index_stats tool.
pub fn get_index_stats(project: &Project) -> Result<CallToolResult, McpError> {
    match void_stack_core::vector_index::get_index_stats(project) {
        Ok(Some(stats)) => {
            let json = to_json_pretty(&stats)?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
        Ok(None) => Ok(CallToolResult::success(vec![Content::text(format!(
            "No index found for project '{}'. Run index_project_codebase first.",
            project.name
        ))])),
        Err(e) => Err(McpError::internal_error(
            format!("Failed to load index stats: {}", e),
            None,
        )),
    }
}
