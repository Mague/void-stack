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

#[cfg(test)]
mod tests {
    use super::*;
    use void_stack_core::global_config::{GlobalConfig, save_global_config};
    use void_stack_core::model::Project;

    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    /// doctor() loads the (isolated) global config and reports registry
    /// issues. A project whose path no longer exists must surface as an issue,
    /// and the JSON must reflect the number of checked projects.
    #[tokio::test]
    async fn test_doctor_reports_missing_path() {
        crate::tools::isolate_test_data_dir();
        let _guard = crate::tools::config_test_guard().await;

        let config = GlobalConfig {
            projects: vec![Project {
                name: format!("doctor-missing-{}", std::process::id()),
                description: String::new(),
                path: "Z:\\definitely\\missing\\path\\xyz".to_string(),
                project_type: None,
                tags: vec![],
                services: vec![],
                hooks: None,
            }],
            ..Default::default()
        };
        save_global_config(&config).unwrap();

        let out = text_of(&doctor().await.unwrap());
        // Report is valid JSON with the expected shape.
        let report: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(report["checked_projects"].as_u64(), Some(1));
        let issues = report["issues"].as_array().expect("issues array");
        assert!(
            issues.iter().any(|i| i["kind"] == "MissingPath"
                || i["detail"]
                    .as_str()
                    .unwrap_or("")
                    .contains("no longer exists")),
            "expected a missing-path issue, got: {out}"
        );
    }
}
