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
struct DiagramRequest {
    /// Name of the project (case-insensitive)
    project: String,
    /// Output format: "mermaid" or "drawio" (default: drawio)
    format: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct AnalyzeRequest {
    /// Name of the project (case-insensitive)
    project: String,
    /// Specific service to analyze (omit for all services)
    service: Option<String>,
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
        // Block sensitive files (secrets, credentials, .env)
        if devlaunch_core::security::is_sensitive_file(&doc_path) {
            return Err(McpError::invalid_params(
                "Cannot read sensitive/credential files".to_string(),
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

    #[tool(description = "Read ALL documentation files from a project at once (README.md, CHANGELOG.md, CLAUDE.md, etc.). Returns all found doc files concatenated. Use this at the start of a conversation to quickly understand a project.")]
    async fn read_all_docs(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let root = devlaunch_core::runner::local::strip_win_prefix(&project.path);
        let doc_extensions = ["md", "txt"];
        let mut docs = Vec::new();
        let mut total_size = 0usize;
        let max_total = 100_000; // 100KB total limit

        // Scan root directory for doc files
        if let Ok(entries) = std::fs::read_dir(&root) {
            let mut files: Vec<_> = entries.flatten()
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let ext = std::path::Path::new(&name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    doc_extensions.contains(&ext)
                })
                .collect();
            files.sort_by_key(|e| e.file_name());

            // Prioritize important files first
            let priority = ["README.md", "CLAUDE.md", "CHANGELOG.md", "CONTRIBUTING.md"];
            files.sort_by_key(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let idx = priority.iter().position(|p| p.eq_ignore_ascii_case(&name));
                idx.unwrap_or(priority.len())
            });

            for entry in files {
                if total_size >= max_total {
                    docs.push(format!("\n---\n[Truncated: reached {}KB limit]\n", max_total / 1000));
                    break;
                }
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let remaining = max_total - total_size;
                    let truncated = if content.len() > remaining {
                        format!("{}...\n[truncated, {} bytes total]", &content[..remaining], content.len())
                    } else {
                        content.clone()
                    };
                    total_size += truncated.len();
                    docs.push(format!("# === {} ===\n\n{}\n", name, truncated));
                }
            }
        }

        // Also check for devlaunch-analysis.md
        let analysis_path = std::path::Path::new(&root).join("devlaunch-analysis.md");
        if analysis_path.exists() && total_size < max_total {
            if let Ok(content) = std::fs::read_to_string(&analysis_path) {
                let remaining = max_total - total_size;
                let truncated = if content.len() > remaining {
                    format!("{}...\n[truncated]", &content[..remaining])
                } else {
                    content
                };
                docs.push(format!("# === devlaunch-analysis.md ===\n\n{}\n", truncated));
            }
        }

        if docs.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                format!("No documentation files found in '{}'.", project.name)
            )]));
        }

        Ok(CallToolResult::success(vec![Content::text(docs.join("\n---\n\n"))]))
    }

    #[tool(description = "Generate architecture, API routes, and DB model diagrams for a project. Supports 'mermaid' (returns markdown) and 'drawio' (saves .drawio file to project dir and returns path). Default format is drawio.")]
    async fn generate_diagram(
        &self,
        params: Parameters<DiagramRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let format = params.0.format.as_deref().unwrap_or("drawio");
        let is_drawio = format.eq_ignore_ascii_case("drawio") || format.eq_ignore_ascii_case("draw.io");

        if is_drawio {
            let xml = devlaunch_core::diagram::drawio::generate_all(&project);
            let dir = devlaunch_core::runner::local::strip_win_prefix(&project.path);
            let path = format!("{}/devlaunch-diagrams.drawio", dir);
            std::fs::write(&path, &xml).map_err(|e| {
                McpError::internal_error(format!("Failed to write drawio file: {}", e), None)
            })?;
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Draw.io diagram saved to: {}\n\nOpen it with VS Code Draw.io extension or at diagrams.net",
                path
            ))]))
        } else {
            let diagrams = devlaunch_core::diagram::generate_all(&project);
            let mut content = format!("# {} — Architecture\n\n## Service Architecture\n\n{}\n\n", project.name, diagrams.architecture);
            if let Some(api) = &diagrams.api_routes {
                content.push_str(&format!("## API Routes\n\n{}\n\n", api));
            }
            if let Some(db) = &diagrams.db_models {
                content.push_str(&format!("## Database Models\n\n{}\n\n", db));
            }
            if !diagrams.warnings.is_empty() {
                content.push_str("## Advertencias\n\n");
                for w in &diagrams.warnings {
                    content.push_str(&format!("- {}\n", w));
                }
                content.push_str("\n");
            }
            Ok(CallToolResult::success(vec![Content::text(content)]))
        }
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

    #[tool(description = "Analyze code architecture: dependency graph, architecture patterns (MVC, Layered, Clean, Monolith), anti-patterns (god class, circular deps, fat controllers, excessive coupling). Returns markdown documentation. Optionally specify a service name to analyze a single service.")]
    async fn analyze_project(
        &self,
        params: Parameters<AnalyzeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let mut results = Vec::new();
        let services: Vec<_> = match &params.0.service {
            Some(svc_name) => {
                project.services.iter()
                    .filter(|s| s.name.eq_ignore_ascii_case(svc_name))
                    .collect()
            }
            None => project.services.iter().collect(),
        };

        for svc in &services {
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let clean = devlaunch_core::runner::local::strip_win_prefix(dir);
            let path = std::path::Path::new(&clean);
            if let Some(result) = devlaunch_core::analyzer::analyze_project(path) {
                let doc = devlaunch_core::analyzer::generate_docs(&result, &svc.name);
                results.push(doc);
            }
        }

        if results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No analyzable code found (supported: Python, JavaScript/TypeScript)".to_string()
            )]));
        }

        let full = results.join("\n\n---\n\n");

        // Save to project dir
        let dir = devlaunch_core::runner::local::strip_win_prefix(&project.path);
        let path = format!("{}/devlaunch-analysis.md", dir);
        let _ = std::fs::write(&path, &full);

        Ok(CallToolResult::success(vec![Content::text(full)]))
    }

    #[tool(description = "Run security audit on a project: scan for vulnerable dependencies (npm audit, pip audit, cargo audit), hardcoded secrets (API keys, tokens, passwords), and insecure configurations (debug mode, open CORS, Docker issues). Returns findings with severity, description, and remediation steps.")]
    async fn audit_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let clean_path = devlaunch_core::runner::local::strip_win_prefix(&project.path);
        let result = devlaunch_core::audit::audit_project(&project.name, std::path::Path::new(&clean_path));

        let json = serde_json::to_string_pretty(&result)
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
