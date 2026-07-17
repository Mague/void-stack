//! daily_briefing tool: consolidated report for the active projects.

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use crate::server::VoidStackMcp;
use crate::types::DailyBriefingRequest;

/// Logic for daily_briefing tool. Runs audits/analysis per project, so it
/// executes on a blocking thread; can take a while on big active lists.
pub async fn daily_briefing(req: DailyBriefingRequest) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let save = req.save.unwrap_or(false);

    let md = tokio::task::spawn_blocking(move || {
        let only = req.projects.as_deref().filter(|p| !p.is_empty());
        let md = void_stack_core::briefing::generate_briefing(&config, only)?;
        if save {
            void_stack_core::briefing::save_briefing(&md, chrono::Local::now().date_naive())?;
        }
        Ok::<String, String>(md)
    })
    .await
    .map_err(|e| McpError::internal_error(format!("briefing task failed: {}", e), None))?
    .map_err(|e| McpError::invalid_params(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(md)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no active projects configured and no explicit project list, the
    /// core briefing generator errors, which the handler surfaces as an
    /// invalid_params MCP error. Fast: nothing is scanned.
    #[tokio::test]
    async fn test_daily_briefing_no_active_projects_errors() {
        let _guard = crate::tools::config_test_guard().await;
        crate::tools::isolate_test_data_dir();
        // Empty explicit list also falls back to the (empty) active list.
        let req = DailyBriefingRequest {
            projects: Some(vec![]),
            save: Some(false),
        };
        let err = daily_briefing(req).await.unwrap_err();
        assert!(
            err.message.contains("no active projects"),
            "unexpected error: {}",
            err.message
        );
    }
}
