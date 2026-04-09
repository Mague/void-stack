use rmcp::ErrorData as McpError;
use rmcp::model::*;

#[cfg(feature = "vector")]
use super::to_json_pretty;
#[cfg(feature = "vector")]
use void_stack_core::model::Project;

/// Logic for index_project_codebase tool.
#[cfg(feature = "vector")]
pub fn index_project_codebase(project: &Project, force: bool) -> Result<CallToolResult, McpError> {
    let stats = void_stack_core::vector_index::index_project(project, force, |_, _| {})
        .map_err(|e| McpError::internal_error(format!("Indexing failed: {}", e), None))?;

    let json = to_json_pretty(&stats)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

#[cfg(not(feature = "vector"))]
pub fn index_project_codebase(
    _project: &void_stack_core::model::Project,
    _force: bool,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

/// Logic for semantic_search tool.
#[cfg(feature = "vector")]
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

#[cfg(not(feature = "vector"))]
pub fn semantic_search(
    _project: &void_stack_core::model::Project,
    _query: &str,
    _top_k: usize,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

/// Logic for generate_voidignore tool (vector-index-aware).
#[cfg(feature = "vector")]
pub fn generate_voidignore(project: &Project) -> Result<CallToolResult, McpError> {
    let result = void_stack_core::vector_index::generate_voidignore(&project.path);
    void_stack_core::vector_index::save_voidignore(&project.path, &result.content).map_err(
        |e| McpError::internal_error(format!("Failed to save .voidignore: {}", e), None),
    )?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Generated .voidignore ({} patterns) for project '{}'.\nContent:\n{}",
        result.patterns_count, project.name, result.content
    ))]))
}

#[cfg(not(feature = "vector"))]
pub fn generate_voidignore(
    _project: &void_stack_core::model::Project,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

/// Logic for get_index_stats tool.
#[cfg(feature = "vector")]
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

#[cfg(not(feature = "vector"))]
pub fn get_index_stats(
    _project: &void_stack_core::model::Project,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}
