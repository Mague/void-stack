use rmcp::ErrorData as McpError;
use rmcp::model::*;
use tracing::info;

use void_stack_core::model::Project;

use super::to_json_pretty;
use crate::server::VoidStackMcp;
use crate::types::{ServiceStateInfo, StartStopResult};

/// Logic for project_status tool.
pub async fn project_status(
    mcp: &VoidStackMcp,
    project: &Project,
) -> Result<CallToolResult, McpError> {
    let mgr = mcp.get_manager(project).await;

    let states = mgr.get_states().await;
    let result: Vec<ServiceStateInfo> = states
        .iter()
        .map(|s| ServiceStateInfo {
            name: s.service_name.clone(),
            status: s.status.to_string(),
            pid: s.pid,
            url: s.url.clone(),
            last_log: s.last_log_line.clone(),
        })
        .collect();

    let json = to_json_pretty(&result)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Logic for start_project tool.
pub async fn start_project(
    mcp: &VoidStackMcp,
    project: &Project,
) -> Result<CallToolResult, McpError> {
    let mgr = mcp.get_manager(project).await;

    info!(project = %project.name, "MCP: Starting all services");

    let states = mgr
        .start_all()
        .await
        .map_err(|e| McpError::internal_error(format!("Start failed: {}", e), None))?;

    let result = StartStopResult {
        project: project.name.clone(),
        results: states
            .iter()
            .map(|s| ServiceStateInfo {
                name: s.service_name.clone(),
                status: s.status.to_string(),
                pid: s.pid,
                url: s.url.clone(),
                last_log: s.last_log_line.clone(),
            })
            .collect(),
    };

    let json = to_json_pretty(&result)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Logic for stop_project tool.
pub async fn stop_project(
    mcp: &VoidStackMcp,
    project: &Project,
) -> Result<CallToolResult, McpError> {
    let mgr = mcp.get_manager(project).await;

    info!(project = %project.name, "MCP: Stopping all services");

    mgr.stop_all()
        .await
        .map_err(|e| McpError::internal_error(format!("Stop failed: {}", e), None))?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "All services in '{}' stopped.",
        project.name,
    ))]))
}

/// Logic for start_service tool.
pub async fn start_service(
    mcp: &VoidStackMcp,
    project: &Project,
    service_name: &str,
) -> Result<CallToolResult, McpError> {
    let mgr = mcp.get_manager(project).await;

    info!(
        project = %project.name,
        service = %service_name,
        "MCP: Starting service"
    );

    let state = mgr
        .start_one(service_name)
        .await
        .map_err(|e| McpError::internal_error(format!("Start failed: {}", e), None))?;

    let info = ServiceStateInfo {
        name: state.service_name.clone(),
        status: state.status.to_string(),
        pid: state.pid,
        url: state.url.clone(),
        last_log: state.last_log_line.clone(),
    };

    let json = to_json_pretty(&info)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Logic for stop_service tool.
pub async fn stop_service(
    mcp: &VoidStackMcp,
    project: &Project,
    service_name: &str,
) -> Result<CallToolResult, McpError> {
    let mgr = mcp.get_manager(project).await;

    info!(
        project = %project.name,
        service = %service_name,
        "MCP: Stopping service"
    );

    mgr.stop_one(service_name)
        .await
        .map_err(|e| McpError::internal_error(format!("Stop failed: {}", e), None))?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Service '{}' in '{}' stopped.",
        service_name, project.name,
    ))]))
}

/// Logic for get_logs tool.
pub async fn get_logs(
    mcp: &VoidStackMcp,
    project: &Project,
    service_name: &str,
    lines: usize,
    raw: bool,
) -> Result<CallToolResult, McpError> {
    let mgr = mcp.get_manager(project).await;

    let all_logs = mgr.get_logs(service_name).await;
    let lines = lines.clamp(1, 500);
    let start = all_logs.len().saturating_sub(lines);
    let recent: Vec<&String> = all_logs[start..].iter().collect();

    if recent.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No logs captured for service '{}'.",
            service_name
        ))]));
    }

    let joined = recent
        .iter()
        .map(|l| l.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    if raw {
        return Ok(CallToolResult::success(vec![Content::text(joined)]));
    }

    // Auto-filter with compact mode for token savings
    let result =
        void_stack_core::log_filter::filter_log_output_tracked(&joined, true, &project.name);
    let output = format!(
        "{}\n\n---\nlines_original: {} | lines_filtered: {} | savings: {:.0}%",
        result.content, result.lines_original, result.lines_filtered, result.savings_pct
    );

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use void_stack_core::model::{Service, Target};

    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    fn service(name: &str) -> Service {
        Service {
            name: name.to_string(),
            command: "echo hi".to_string(),
            target: Target::Windows,
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
            docker: None,
        }
    }

    fn project_with(services: Vec<Service>) -> Project {
        Project {
            name: "svc-fixture".to_string(),
            description: String::new(),
            path: ".".to_string(),
            project_type: None,
            tags: vec![],
            services,
            hooks: None,
        }
    }

    /// Freshly built manager reports every service as STOPPED with no PID.
    #[tokio::test]
    async fn test_project_status_reports_stopped_states() {
        let mcp = VoidStackMcp::new();
        let project = project_with(vec![service("api"), service("web")]);

        let out = text_of(&project_status(&mcp, &project).await.unwrap());
        let states: serde_json::Value = serde_json::from_str(&out).unwrap();
        let arr = states.as_array().expect("states array");
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().all(|s| s["status"] == "STOPPED"));
        assert!(arr.iter().any(|s| s["name"] == "api"));
        assert!(arr.iter().any(|s| s["name"] == "web"));
    }

    /// stop_project on nothing running succeeds with a confirmation message.
    #[tokio::test]
    async fn test_stop_project_when_idle() {
        let mcp = VoidStackMcp::new();
        let project = project_with(vec![service("api")]);

        let out = text_of(&stop_project(&mcp, &project).await.unwrap());
        assert!(out.contains("stopped"), "got: {out}");
        assert!(out.contains("svc-fixture"), "got: {out}");
    }

    /// get_logs with no captured output returns the empty-log notice.
    #[tokio::test]
    async fn test_get_logs_no_output() {
        let mcp = VoidStackMcp::new();
        let project = project_with(vec![service("api")]);

        let out = text_of(&get_logs(&mcp, &project, "api", 50, false).await.unwrap());
        assert!(out.contains("No logs captured"), "got: {out}");
    }
}
