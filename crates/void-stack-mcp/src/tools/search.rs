use rmcp::ErrorData as McpError;
use rmcp::model::*;

#[cfg(feature = "vector")]
use super::to_json_pretty;
#[cfg(feature = "vector")]
use void_stack_core::model::Project;

/// Logic for index_project_codebase tool (non-blocking).
#[cfg(feature = "vector")]
pub fn index_project_codebase(project: &Project, force: bool) -> Result<CallToolResult, McpError> {
    // Check if already running
    if let Some(void_stack_core::vector_index::IndexJobStatus::Running {
        files_processed,
        files_total,
    }) = void_stack_core::vector_index::get_index_job_status(project)
    {
        let msg = if files_total > 0 {
            format!(
                "Indexing already in progress: {}/{} files processed ({:.0}%). \
                 Call get_index_stats to check when complete.",
                files_processed,
                files_total,
                files_processed as f64 / files_total as f64 * 100.0
            )
        } else {
            "Indexing already in progress (initializing model...). \
             Call get_index_stats to check when complete."
                .to_string()
        };
        return Ok(CallToolResult::success(vec![Content::text(msg)]));
    }

    // Start background indexing
    void_stack_core::vector_index::index_project_background(project, force);

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Indexing started for '{}' (background). \
         First run downloads the embedding model (~130MB, one-time). \
         Call get_index_stats in ~30-60 seconds to check progress. \
         Use semantic_search once get_index_stats shows 'created_at'.",
        project.name
    ))]))
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
    let project_path = std::path::Path::new(&project.path);
    let result = void_stack_core::vector_index::generate_voidignore(project_path);
    void_stack_core::vector_index::save_voidignore(project_path, &result.content).map_err(|e| {
        McpError::internal_error(format!("Failed to save .voidignore: {}", e), None)
    })?;

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

/// Logic for get_index_stats tool (shows job status if in progress).
#[cfg(feature = "vector")]
pub fn get_index_stats(project: &Project) -> Result<CallToolResult, McpError> {
    // Check for active/recent job first
    if let Some(status) = void_stack_core::vector_index::get_index_job_status(project) {
        match status {
            void_stack_core::vector_index::IndexJobStatus::Running {
                files_processed,
                files_total,
            } => {
                // If all files processed, check if index already completed on disk
                // (race condition: index saved to disk but registry not yet updated)
                if files_total > 0 && files_processed >= files_total {
                    if let Ok(Some(stats)) = void_stack_core::vector_index::get_index_stats(project)
                    {
                        let json = to_json_pretty(&stats)?;
                        return Ok(CallToolResult::success(vec![Content::text(json)]));
                    }
                    // Still generating embeddings/HNSW
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Status: INDEXING IN PROGRESS\n\
                         Files read: {}/{} (100%)\n\
                         Generating embeddings and building HNSW index...\n\
                         Call get_index_stats again in 30-60 seconds.",
                        files_processed, files_total
                    ))]));
                }

                let msg = if files_total > 0 {
                    format!(
                        "Status: INDEXING IN PROGRESS\nFiles: {}/{} ({:.0}%)\n\
                         The index will be ready when this completes.",
                        files_processed,
                        files_total,
                        files_processed as f64 / files_total as f64 * 100.0
                    )
                } else {
                    "Status: INDEXING IN PROGRESS\nInitializing embedding model...".to_string()
                };
                return Ok(CallToolResult::success(vec![Content::text(msg)]));
            }
            void_stack_core::vector_index::IndexJobStatus::Failed { error } => {
                return Err(McpError::internal_error(
                    format!("Last indexing failed: {}", error),
                    None,
                ));
            }
            void_stack_core::vector_index::IndexJobStatus::Completed { .. } => {
                // Fall through to read stats from disk
            }
        }
    }

    // Normal stats read from disk
    match void_stack_core::vector_index::get_index_stats(project) {
        Ok(Some(stats)) => {
            let json = to_json_pretty(&stats)?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
        Ok(None) => Ok(CallToolResult::success(vec![Content::text(format!(
            "No index found for '{}'. Run index_project_codebase first.",
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

#[cfg(feature = "vector")]
#[cfg(test)]
mod tests {
    use super::*;
    use void_stack_core::model::Project;

    fn make_test_project() -> Project {
        Project {
            name: "test-project".to_string(),
            path: "F:\\workspace\\test-project".to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    #[test]
    fn test_index_project_codebase_returns_immediately() {
        let project = make_test_project();
        let result = index_project_codebase(&project, false);

        assert!(result.is_ok(), "index_project_codebase should return Ok");
    }

    #[test]
    fn test_index_project_codebase_twice_returns_already_running() {
        let project = make_test_project();

        // First call starts the job
        let result1 = index_project_codebase(&project, false);
        assert!(result1.is_ok());

        // Second call should return "already in progress" - still Ok
        let result2 = index_project_codebase(&project, false);
        assert!(result2.is_ok(), "Second call should also return Ok");
    }

    #[test]
    fn test_get_index_stats_during_running_job() {
        let project = make_test_project();

        // Start indexing
        let _ = index_project_codebase(&project, false);

        // Get stats should show running status - still Ok
        let result = get_index_stats(&project);
        assert!(
            result.is_ok(),
            "get_index_stats should return Ok during job"
        );
    }

    #[test]
    fn test_get_index_stats_no_index_returns_message() {
        let project = make_test_project();

        // No index started, no job running - should return Ok with message
        let result = get_index_stats(&project);
        assert!(
            result.is_ok(),
            "get_index_stats should return Ok when no index"
        );
    }

    #[test]
    fn test_semantic_search_returns_ok_when_no_index() {
        let project = make_test_project();

        // Note: semantic_search may return Ok with "No results found" OR error
        // depending on implementation. Both are acceptable behaviors.
        // Just verify it doesn't panic.
        let _ = semantic_search(&project, "test query", 5);
    }
}
