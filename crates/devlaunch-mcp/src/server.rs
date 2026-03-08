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
    default_command_for_dir, GlobalConfig,
};
use devlaunch_core::manager::ProcessManager;
use devlaunch_core::model::{Project, Service, Target};

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

#[derive(Deserialize, JsonSchema)]
struct ReadDocsRequest {
    /// Name of the project
    project: String,
    /// Filename to read (default: README.md). Supports: README.md, CHANGELOG.md, CLAUDE.md, etc.
    #[serde(default = "default_doc_file")]
    filename: String,
}

fn default_doc_file() -> String {
    "README.md".to_string()
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

    #[tool(description = "Start all services in a project. Returns immediately. Use project_status afterwards to get detected URLs.")]
    async fn start_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        let mgr = self.get_manager(&project).await;

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

    #[tool(description = "Start a specific service within a project. Use project_status afterwards to get the detected URL.")]
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

        let state = mgr
            .start_one(&params.0.service)
            .await
            .map_err(|e| McpError::internal_error(format!("Start failed: {}", e), None))?;

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
                command: default_command_for_dir(*pt, sub_path),
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

    #[tool(description = "Read documentation files (README.md, CHANGELOG.md, CLAUDE.md, etc.) from a project directory to understand what the project does")]
    async fn read_project_docs(
        &self,
        params: Parameters<ReadDocsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let root = devlaunch_core::runner::local::strip_win_prefix(&project.path);
        let doc_path = std::path::Path::new(&root).join(&params.0.filename);

        // Security: only allow reading markdown/text files within the project
        let allowed_extensions = ["md", "txt", "toml", "json", "yml", "yaml"];
        let ext = doc_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if !allowed_extensions.contains(&ext) {
            return Err(McpError::invalid_params(
                format!("File type '.{}' not allowed. Use: {}", ext, allowed_extensions.join(", ")),
                None,
            ));
        }

        match std::fs::read_to_string(&doc_path) {
            Ok(content) => {
                // Truncate very large files
                let truncated = if content.len() > 50_000 {
                    format!("{}...\n\n[truncated, {} bytes total]", &content[..50_000], content.len())
                } else {
                    content
                };
                Ok(CallToolResult::success(vec![Content::text(truncated)]))
            }
            Err(_) => {
                // List available doc files
                let available = list_doc_files(&root);
                let msg = if available.is_empty() {
                    format!("'{}' not found in '{}'. No documentation files found.", params.0.filename, project.name)
                } else {
                    format!(
                        "'{}' not found in '{}'. Available files:\n{}",
                        params.0.filename,
                        project.name,
                        available.join("\n")
                    )
                };
                Ok(CallToolResult::success(vec![Content::text(msg)]))
            }
        }
    }

    #[tool(description = "Generate Mermaid architecture, API routes, and DB model diagrams for a project. Returns markdown with embedded Mermaid code blocks.")]
    async fn generate_diagram(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let diagrams = devlaunch_core::diagram::generate_all(&project);
        let mut content = format!("# {} — Architecture\n\n## Service Architecture\n\n{}\n\n", project.name, diagrams.architecture);
        if let Some(api) = &diagrams.api_routes {
            content.push_str(&format!("## API Routes\n\n{}\n\n", api));
        }
        if let Some(db) = &diagrams.db_models {
            content.push_str(&format!("## Database Models\n\n{}\n\n", db));
        }
        Ok(CallToolResult::success(vec![Content::text(content)]))
    }

    #[tool(description = "Check all dependencies for a project (Python, Node, CUDA, Ollama, Docker, .env). Returns status, versions, and fix hints for each dependency.")]
    async fn check_dependencies(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        // Collect all unique directories
        let mut dirs: Vec<std::path::PathBuf> = vec![];
        let root = devlaunch_core::runner::local::strip_win_prefix(&project.path);
        dirs.push(std::path::PathBuf::from(&root));

        for svc in &project.services {
            if let Some(dir) = &svc.working_dir {
                let stripped = devlaunch_core::runner::local::strip_win_prefix(dir);
                let p = std::path::PathBuf::from(&stripped);
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }

        let mut seen = std::collections::HashSet::new();
        let mut all_results = Vec::new();

        for dir in &dirs {
            let results = devlaunch_core::detector::check_project(dir).await;
            for result in results {
                if seen.insert(format!("{:?}", result.dep_type)) {
                    all_results.push(result);
                }
            }
        }

        let json = serde_json::to_string_pretty(&all_results)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
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

/// List documentation files in a project directory.
fn list_doc_files(root: &str) -> Vec<String> {
    let path = std::path::Path::new(root);
    let doc_extensions = ["md", "txt"];
    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(ext) = std::path::Path::new(&name).extension().and_then(|e| e.to_str()) {
                if doc_extensions.contains(&ext) {
                    files.push(format!("  - {}", name));
                }
            }
        }
    }
    files.sort();
    files
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
