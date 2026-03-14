use rmcp::ErrorData as McpError;
use rmcp::model::*;
use tracing::info;

use void_stack_core::model::Project;

use super::to_json_pretty;
use crate::server::{ServiceStateInfo, StartStopResult, VoidStackMcp};

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
) -> Result<CallToolResult, McpError> {
    let mgr = mcp.get_manager(project).await;

    let all_logs = mgr.get_logs(service_name).await;
    let lines = lines.clamp(1, 500);
    let start = all_logs.len().saturating_sub(lines);
    let recent: Vec<&String> = all_logs[start..].iter().collect();

    let output = if recent.is_empty() {
        format!("No logs captured for service '{}'.", service_name)
    } else {
        recent
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
