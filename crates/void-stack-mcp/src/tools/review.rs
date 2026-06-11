//! Diff-centric tools: `suggest_tests_for_diff` and `review_diff`.

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use crate::server::VoidStackMcp;
use crate::types::{DeadCodeRequest, ReviewDiffRequest, SuggestTestsRequest};

#[cfg(feature = "structural")]
pub async fn suggest_tests_for_diff(
    _mcp: &VoidStackMcp,
    req: SuggestTestsRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;
    let max = req.max_results.unwrap_or(20);
    let base = req.git_base.clone();

    let suggestions = tokio::task::spawn_blocking(move || {
        void_stack_core::testing::suggest_tests_for_diff(&project, base.as_deref(), max)
    })
    .await
    .map_err(|e| McpError::internal_error(format!("suggest task failed: {}", e), None))?
    .map_err(|e| McpError::internal_error(format!("suggest_tests_for_diff failed: {}", e), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        void_stack_core::testing::render_suggestions_markdown(&suggestions),
    )]))
}

#[cfg(not(feature = "structural"))]
pub async fn suggest_tests_for_diff(
    _mcp: &VoidStackMcp,
    _req: SuggestTestsRequest,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "suggest_tests_for_diff requires the `structural` feature".to_string(),
        None,
    ))
}

#[cfg(feature = "structural")]
pub async fn review_diff(
    _mcp: &VoidStackMcp,
    req: ReviewDiffRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;
    let base = req.git_base.clone();

    let payload = tokio::task::spawn_blocking(move || {
        void_stack_core::review::review_diff(&project, base.as_deref())
    })
    .await
    .map_err(|e| McpError::internal_error(format!("review task failed: {}", e), None))?
    .map_err(|e| McpError::internal_error(format!("review_diff failed: {}", e), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        payload.markdown,
    )]))
}

#[cfg(not(feature = "structural"))]
pub async fn review_diff(
    _mcp: &VoidStackMcp,
    _req: ReviewDiffRequest,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "review_diff requires the `structural` feature".to_string(),
        None,
    ))
}

#[cfg(feature = "structural")]
pub async fn find_dead_code(
    _mcp: &VoidStackMcp,
    req: DeadCodeRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;
    let max = req.max_results.unwrap_or(50);

    let report = tokio::task::spawn_blocking(move || {
        void_stack_core::deadcode::find_dead_code(&project, max)
    })
    .await
    .map_err(|e| McpError::internal_error(format!("dead-code task failed: {}", e), None))?
    .map_err(|e| McpError::internal_error(format!("find_dead_code failed: {}", e), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        void_stack_core::deadcode::render_dead_code_markdown(&report),
    )]))
}

#[cfg(not(feature = "structural"))]
pub async fn find_dead_code(
    _mcp: &VoidStackMcp,
    _req: DeadCodeRequest,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "find_dead_code requires the `structural` feature".to_string(),
        None,
    ))
}
