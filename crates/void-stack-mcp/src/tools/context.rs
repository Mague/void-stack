//! session_context tool: one-call session bootstrap for LLM agents.

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;

/// Logic for session_context tool. Filesystem + git + SQLite work, so it
/// runs on a blocking thread like review_diff.
pub async fn session_context(project: Project) -> Result<CallToolResult, McpError> {
    let md =
        tokio::task::spawn_blocking(move || void_stack_core::context::session_context(&project))
            .await
            .map_err(|e| McpError::internal_error(format!("context task failed: {}", e), None))?
            .map_err(|e| McpError::internal_error(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(md)]))
}
