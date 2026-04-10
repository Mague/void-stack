use rmcp::ErrorData as McpError;
use rmcp::model::*;

use super::to_json_pretty;

/// Logic for get_token_stats tool.
pub fn get_token_stats(project: Option<&str>, days: u32) -> Result<CallToolResult, McpError> {
    let report = void_stack_core::stats::get_stats(project, days)
        .map_err(|e| McpError::internal_error(format!("Failed to load stats: {}", e), None))?;

    let json = to_json_pretty(&report)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}
