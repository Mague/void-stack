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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_token_stats_returns_json_report() {
        // Isolate central state: the stats DB is created inside the
        // per-process test data dir, not the user's real one.
        crate::tools::isolate_test_data_dir();

        let result = get_token_stats(None, 30).unwrap();
        let text = result.content[0].as_text().expect("text result");
        let v: serde_json::Value = serde_json::from_str(&text.text).expect("valid JSON");
        assert!(v.is_object(), "report must be a JSON object");

        // Filtering by an unknown project also yields a valid (empty) report.
        let result = get_token_stats(Some("no-such-project"), 7).unwrap();
        let text = result.content[0].as_text().expect("text result");
        assert!(serde_json::from_str::<serde_json::Value>(&text.text).is_ok());
    }
}
