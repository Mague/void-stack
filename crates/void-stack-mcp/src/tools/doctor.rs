//! doctor tool: read-only registry sanity report.

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use crate::server::VoidStackMcp;

use super::to_json_pretty;

/// Logic for doctor tool. Read-only by design — fixes are applied from the
/// CLI (`void doctor --fix`) where a human confirms each one.
pub async fn doctor() -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let report = tokio::task::spawn_blocking(move || {
        void_stack_core::doctor::run_doctor(&config, &void_stack_core::doctor::indexes_root())
    })
    .await
    .map_err(|e| McpError::internal_error(format!("doctor task failed: {}", e), None))?;

    let json = to_json_pretty(&report)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}
