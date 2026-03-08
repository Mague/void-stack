use std::collections::HashMap;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::info;

use devlaunch_core::global_config::{
    load_global_config, save_global_config, find_project, remove_project, scan_subprojects,
    default_command_for, GlobalConfig,
};
use devlaunch_core::manager::ProcessManager;
use devlaunch_core::model::{Project, Service, ServiceStatus, Target};

// ── Tool parameter types ────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
struct ProjectName {
    /// Name of the project (case-insensitive)
    project: String,
}

#[derive(Deserialize, JsonSchema)]
struct ServiceRef {
    /// Name of the project
    project: String,
    /// Name of the service within the project
    service: String,
}

#[derive(Deserialize, JsonSchema)]
struct LogsRequest {
    /// Name of the project
    project: String,
    /// Name of the service
    service: String,
    /// Maximum number of log lines to return (default: 50)
    #[serde(default = "default_log_lines")]
    lines: usize,
}

fn default_log_lines() -> usize {
    50
}

#[derive(Deserialize, JsonSchema)]
struct AddProjectRequest {
    /// Name for the project
    name: String,
    /// Absolute path to the project directory
    path: String,
}

// ── Response types ──────────────────────────────────────────

#[derive(Serialize)]
struct ProjectInfo {
    name: String,
    path: String,
    project_type: String,
    services: Vec<ServiceInfo>,
}

#[derive(Serialize)]
struct ServiceInfo {
    name: String,
    command: String,
    target: String,
    working_dir: Option<String>,
    enabled: bool,
}

#[derive(Serialize)]
struct ServiceStateInfo {
    name: String,
    status: String,
    pid: Option<u32>,
    url: Option<String>,
    last_log: Option<String>,
}

#[derive(Serialize)]
struct StartStopResult {
    project: String,
    results: Vec<ServiceStateInfo>,
}

// ── MCP Server ──────────────────────────────────────────────

#[derive(Clone)]
pub struct DevLaunchMcp {
    /// Active ProcessManagers keyed by project name
    managers: Arc<Mutex<HashMap<String, Arc<ProcessManager>>>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl DevLaunchMcp {
    pub fn new() -> Self {
        Self {
            managers: Arc::new(Mutex::new(HashMap::new())),
            tool_router: Self::tool_router(),
        }
    }

    /// Get or create a ProcessManager for a project
    async fn get_manager(&self, project: &Project) -> Arc<ProcessManager> {
        let mut managers = self.managers.lock().await;
        if let Some(mgr) = managers.get(&project.name) {
            return Arc::clone(mgr);
        }
        let mgr = Arc::new(ProcessManager::new(project.clone()));
        managers.insert(project.name.clone(), Arc::clone(&mgr));
        mgr
    }

    fn load_config() -> Result<GlobalConfig, McpError> {
        load_global_config().map_err(|e| {
            McpError::internal_error(format!("Failed to load config: {}", e), None)
        })
    }

    fn find_project_or_err(config: &GlobalConfig, name: &str) -> Result<Project, McpError> {
        find_project(config, name)
            .cloned()
            .ok_or_else(|| McpError::invalid_params(format!("Project '{}' not found", name), None))
    }

    // ── Tools ───────────────────────────────────────────────

    #[tool(description = "List all registered DevLaunch projects with their services")]
    async fn list_projects(&self) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;

        let projects: Vec<ProjectInfo> = config
            .projects
            .iter()
            .map(|p| ProjectInfo {
                name: p.name.clone(),
                path: p.path.clone(),
                project_type: p
                    .project_type
                    .map(|t| format!("{:?}", t))
                    .unwrap_or_else(|| "Unknown".into()),
                services: p
                    .services
                    .iter()
                    .map(|s| ServiceInfo {
                        name: s.name.clone(),
                        command: s.command.clone(),
                        target: s.target.to_string(),
                        working_dir: s.working_dir.clone(),
                        enabled: s.enabled,
                    })
                    .collect(),
            })
            .collect();

        let json = serde_json::to_string_pretty(&projects)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Get the live status of all services in a project (running, stopped, PIDs, URLs)")]
    async fn project_status(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        let mgr = self.get_manager(&project).await;

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

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Start all services in a project. Waits up to 10s for URLs. For slow services, use project_status to check URLs later.")]
    async fn start_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        let mgr = self.get_manager(&project).await;

        info!(project = %project.name, "MCP: Starting all services");

        let running_count = {
            let states = mgr
                .start_all()
                .await
                .map_err(|e| McpError::internal_error(format!("Start failed: {}", e), None))?;
            states.iter().filter(|s| s.status == ServiceStatus::Running).count()
        };

        // Brief poll for URLs (5 checks × 2s = 10s max)
        // Fast services like Vite respond in ~1s; slow ones can be queried with project_status
        for _ in 0..5 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let current = mgr.get_states().await;
            let urls_found = current.iter().filter(|s| s.url.is_some()).count();
            if urls_found >= running_count {
                break;
            }
        }

        // Return final state with detected URLs
        let final_states = mgr.get_states().await;
        let result = StartStopResult {
            project: project.name.clone(),
            results: final_states
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

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Stop all services in a project")]
    async fn stop_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        let mgr = self.get_manager(&project).await;

        info!(project = %project.name, "MCP: Stopping all services");

        mgr.stop_all()
            .await
            .map_err(|e| McpError::internal_error(format!("Stop failed: {}", e), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "All services in '{}' stopped.",
            project.name,
        ))]))
    }

    #[tool(description = "Start a specific service within a project. Waits up to 10s for its URL. Use project_status if URL is not yet available.")]
    async fn start_service(
        &self,
        params: Parameters<ServiceRef>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        let mgr = self.get_manager(&project).await;

        info!(
            project = %project.name,
            service = %params.0.service,
            "MCP: Starting service"
        );

        let svc_name = params.0.service.clone();
        mgr.start_one(&svc_name)
            .await
            .map_err(|e| McpError::internal_error(format!("Start failed: {}", e), None))?;

        // Brief poll for URL (5 checks × 2s = 10s max)
        for _ in 0..5 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if let Some(s) = mgr.get_state(&svc_name).await {
                if s.url.is_some() {
                    break;
                }
            }
        }

        let state = mgr.get_state(&svc_name).await.unwrap_or_else(|| {
            devlaunch_core::model::ServiceState::new(svc_name.clone())
        });

        let info = ServiceStateInfo {
            name: state.service_name.clone(),
            status: state.status.to_string(),
            pid: state.pid,
            url: state.url.clone(),
            last_log: state.last_log_line.clone(),
        };

        let json = serde_json::to_string_pretty(&info)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Stop a specific service within a project")]
    async fn stop_service(
        &self,
        params: Parameters<ServiceRef>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        let mgr = self.get_manager(&project).await;

        info!(
            project = %project.name,
            service = %params.0.service,
            "MCP: Stopping service"
        );

        mgr.stop_one(&params.0.service)
            .await
            .map_err(|e| McpError::internal_error(format!("Stop failed: {}", e), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Service '{}' in '{}' stopped.",
            params.0.service, project.name,
        ))]))
    }

    #[tool(description = "Get recent log output from a service")]
    async fn get_logs(
        &self,
        params: Parameters<LogsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        let mgr = self.get_manager(&project).await;

        let all_logs = mgr.get_logs(&params.0.service).await;
        let lines = params.0.lines.max(1).min(500);
        let start = all_logs.len().saturating_sub(lines);
        let recent: Vec<&String> = all_logs[start..].iter().collect();

        let output = if recent.is_empty() {
            format!("No logs captured for service '{}'.", params.0.service)
        } else {
            recent.iter().map(|l| l.as_str()).collect::<Vec<_>>().join("\n")
        };

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "Scan a directory and register it as a DevLaunch project with auto-detected services")]
    async fn add_project(
        &self,
        params: Parameters<AddProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        let path = std::path::Path::new(&params.0.path);
        if !path.exists() {
            return Err(McpError::invalid_params(
                format!("Path '{}' does not exist", params.0.path),
                None,
            ));
        }

        let mut config = Self::load_config()?;

        // Check if already registered
        if find_project(&config, &params.0.name).is_some() {
            return Err(McpError::invalid_params(
                format!("Project '{}' already exists", params.0.name),
                None,
            ));
        }

        // Scan for sub-projects
        let detected = scan_subprojects(path);
        let services: Vec<Service> = detected
            .iter()
            .map(|(name, sub_path, pt)| Service {
                name: name.clone(),
                command: default_command_for(*pt),
                target: Target::Windows,
                working_dir: Some(sub_path.to_string_lossy().to_string()),
                enabled: true,
                env_vars: vec![],
                depends_on: vec![],
            })
            .collect();

        let project_type = detected.first().map(|(_, _, pt)| *pt);

        let project = Project {
            name: params.0.name.clone(),
            description: String::new(),
            path: params.0.path.clone(),
            project_type,
            tags: vec![],
            services: services.clone(),
            hooks: None,
        };

        config.projects.push(project);
        save_global_config(&config)
            .map_err(|e| McpError::internal_error(format!("Failed to save config: {}", e), None))?;

        info!(project = %params.0.name, services = services.len(), "MCP: Project registered");

        let service_list: Vec<String> = services
            .iter()
            .map(|s| format!("  - {} ({})", s.name, s.command))
            .collect();

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Project '{}' registered with {} services:\n{}",
            params.0.name,
            services.len(),
            service_list.join("\n"),
        ))]))
    }

    #[tool(description = "Remove a registered project from DevLaunch")]
    async fn remove_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let mut config = Self::load_config()?;

        // Stop services if running
        if let Some(project) = find_project(&config, &params.0.project).cloned() {
            let mgr = self.get_manager(&project).await;
            let _ = mgr.stop_all().await;

            // Remove from active managers
            let mut managers = self.managers.lock().await;
            managers.remove(&project.name);
        }

        if remove_project(&mut config, &params.0.project) {
            save_global_config(&config)
                .map_err(|e| McpError::internal_error(format!("Failed to save config: {}", e), None))?;
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Project '{}' removed.",
                params.0.project,
            ))]))
        } else {
            Err(McpError::invalid_params(
                format!("Project '{}' not found", params.0.project),
                None,
            ))
        }
    }
}

// ── ServerHandler ───────────────────────────────────────────

#[tool_handler]
impl ServerHandler for DevLaunchMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "DevLaunch MCP server — manage development service projects. \
                 Use list_projects to see registered projects, start_project/stop_project \
                 to manage services, get_logs for output, and add_project to register new ones.",
            )
    }
}
