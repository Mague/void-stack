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

use void_stack_core::global_config::{
    load_global_config, save_global_config, find_project, remove_project, scan_subprojects,
    default_command_for_dir, GlobalConfig,
};
use void_stack_core::manager::ProcessManager;
use void_stack_core::model::{Project, Service, Target};
use void_stack_core::runner::local::strip_win_prefix;

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
    /// Include best practices analysis (ruff, clippy, golangci-lint, react-doctor, dart analyze)
    best_practices: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct AddProjectRequest {
    /// Name for the project
    name: String,
    /// Absolute path to the project directory (Windows path or WSL UNC path like \\\\wsl.localhost\\Ubuntu\\home\\user\\project)
    path: String,
    /// Set to true if the project is inside WSL. When true, provide a Linux path and specify the distro.
    #[serde(default)]
    wsl: bool,
    /// WSL distro name (e.g., "Ubuntu"). Required when wsl=true and path is a Linux path.
    distro: Option<String>,
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

#[derive(Deserialize, JsonSchema)]
struct ScanDirectoryRequest {
    /// Absolute path to the directory to scan
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct AddServiceRequest {
    /// Name of the project to add the service to (case-insensitive)
    project: String,
    /// Name for the new service
    name: String,
    /// Command to run the service (e.g., "npm run dev")
    command: String,
    /// Absolute path to the service's working directory
    working_dir: String,
    /// Execution target: "windows", "wsl", or "docker" (default: windows)
    target: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SaveDebtRequest {
    /// Name of the project (case-insensitive)
    project: String,
    /// Optional label for the snapshot (e.g., git tag or version)
    label: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct CompareDebtRequest {
    /// Name of the project (case-insensitive)
    project: String,
    /// Index of the first snapshot to compare (0-based). Defaults to second-to-last.
    index_a: Option<usize>,
    /// Index of the second snapshot to compare (0-based). Defaults to last.
    index_b: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct DockerGenerateRequest {
    /// Name of the project (case-insensitive)
    project: String,
    /// Generate a Dockerfile if one doesn't exist (default: true)
    generate_dockerfile: Option<bool>,
    /// Generate a docker-compose.yml (default: true)
    generate_compose: Option<bool>,
    /// Save generated files to the project directory (default: false)
    save: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct SuggestRequest {
    /// Name of the project (case-insensitive)
    project: String,
    /// Override model (e.g., "llama3.2", "qwen2.5-coder:7b"). Uses config default if omitted.
    model: Option<String>,
    /// Specific service to analyze (omit for first analyzable service)
    service: Option<String>,
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
pub struct VoidStackMcp {
    /// Active ProcessManagers keyed by project name
    managers: Arc<Mutex<HashMap<String, Arc<ProcessManager>>>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl VoidStackMcp {
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

    #[tool(description = "List all registered VoidStack projects with their services")]
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

    #[tool(description = "Scan a directory and register it as a VoidStack project with auto-detected services. For WSL projects, set wsl=true and provide distro name.")]
    async fn add_project(
        &self,
        params: Parameters<AddProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        use void_stack_core::runner::local::is_wsl_unc_path;

        let is_wsl = params.0.wsl || is_wsl_unc_path(&params.0.path);
        let target = if is_wsl { Target::Wsl } else { Target::Windows };

        // For WSL: convert Linux path to UNC if needed
        let project_path = if is_wsl && !is_wsl_unc_path(&params.0.path) {
            let distro = params.0.distro.as_deref().ok_or_else(|| {
                McpError::invalid_params(
                    "WSL projects require a 'distro' parameter (e.g., \"Ubuntu\")".to_string(),
                    None,
                )
            })?;
            format!(r"\\wsl.localhost\{}{}", distro, params.0.path.replace('/', r"\"))
        } else {
            params.0.path.clone()
        };

        let path = std::path::Path::new(&project_path);
        if !path.exists() {
            return Err(McpError::invalid_params(
                format!("Path '{}' does not exist", project_path),
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
                target,
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
            path: project_path.clone(),
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

        let root = void_stack_core::runner::local::strip_win_prefix(&project.path);
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
        if void_stack_core::security::is_sensitive_file(&doc_path) {
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

        let root = void_stack_core::runner::local::strip_win_prefix(&project.path);
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

        // Also check for void-stack-analysis.md
        let analysis_path = std::path::Path::new(&root).join("void-stack-analysis.md");
        if analysis_path.exists() && total_size < max_total {
            if let Ok(content) = std::fs::read_to_string(&analysis_path) {
                let remaining = max_total - total_size;
                let truncated = if content.len() > remaining {
                    format!("{}...\n[truncated]", &content[..remaining])
                } else {
                    content
                };
                docs.push(format!("# === void-stack-analysis.md ===\n\n{}\n", truncated));
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
            let xml = void_stack_core::diagram::drawio::generate_all(&project);
            let dir = void_stack_core::runner::local::strip_win_prefix(&project.path);
            let path = format!("{}/void-stack-diagrams.drawio", dir);
            std::fs::write(&path, &xml).map_err(|e| {
                McpError::internal_error(format!("Failed to write drawio file: {}", e), None)
            })?;
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Draw.io diagram saved to: {}\n\nOpen it with VS Code Draw.io extension or at diagrams.net",
                path
            ))]))
        } else {
            let diagrams = void_stack_core::diagram::generate_all(&project);
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
        let root = void_stack_core::runner::local::strip_win_prefix(&project.path);
        dirs.push(std::path::PathBuf::from(&root));

        for svc in &project.services {
            if let Some(dir) = &svc.working_dir {
                let stripped = void_stack_core::runner::local::strip_win_prefix(dir);
                let p = std::path::PathBuf::from(&stripped);
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }

        let mut seen = std::collections::HashSet::new();
        let mut all_results = Vec::new();

        for dir in &dirs {
            let results = void_stack_core::detector::check_project(dir).await;
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
            let clean = void_stack_core::runner::local::strip_win_prefix(dir);
            let path = std::path::Path::new(&clean);
            if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
                let doc = void_stack_core::analyzer::generate_docs(&result, &svc.name);
                results.push(doc);
            }
        }

        if results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No analyzable code found (supported: Python, JavaScript/TypeScript)".to_string()
            )]));
        }

        let mut full = results.join("\n\n---\n\n");

        // Best practices analysis if requested
        if params.0.best_practices.unwrap_or(false) {
            let dir = void_stack_core::runner::local::strip_win_prefix(&project.path);
            let bp_result = void_stack_core::analyzer::best_practices::analyze_best_practices(
                std::path::Path::new(&dir)
            );
            let bp_md = void_stack_core::analyzer::best_practices::report::generate_best_practices_markdown(&bp_result);
            full.push_str("\n\n");
            full.push_str(&bp_md);
        }

        // Save to project dir
        let dir = void_stack_core::runner::local::strip_win_prefix(&project.path);
        let path = format!("{}/void-stack-analysis.md", dir);
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

        let clean_path = void_stack_core::runner::local::strip_win_prefix(&project.path);
        let result = void_stack_core::audit::audit_project(&project.name, std::path::Path::new(&clean_path));

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Remove a registered project from VoidStack")]
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

    #[tool(description = "Preview what services would be detected at a directory path, without registering the project. Useful for checking before adding.")]
    async fn scan_directory(
        &self,
        params: Parameters<ScanDirectoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let path_str = strip_win_prefix(&params.0.path);
        let path = std::path::Path::new(&path_str);
        if !path.exists() {
            return Err(McpError::invalid_params(
                format!("Path '{}' does not exist", params.0.path),
                None,
            ));
        }

        let detected = scan_subprojects(path);

        if detected.is_empty() {
            let pt = void_stack_core::config::detect_project_type(path);
            let cmd = default_command_for_dir(pt, path);
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No sub-projects found. Root detected as {:?}.\n\nSuggested service:\n  - name: {}\n  - command: {}\n  - type: {:?}",
                pt,
                path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "main".into()),
                cmd,
                pt,
            ))]));
        }

        let mut lines = vec![format!("Found {} service(s) at '{}':\n", detected.len(), path_str)];
        for (name, sub_path, pt) in &detected {
            let cmd = default_command_for_dir(*pt, sub_path);
            let rel = sub_path.strip_prefix(path).unwrap_or(sub_path);
            lines.push(format!(
                "  - {} ({:?})\n    path: {}\n    command: {}",
                name, pt, rel.display(), cmd,
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(lines.join("\n"))]))
    }

    #[tool(description = "Add a service to an existing registered project. Specify the command, working directory, and optionally the target (windows/wsl/docker).")]
    async fn add_service(
        &self,
        params: Parameters<AddServiceRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mut config = Self::load_config()?;

        let project = config
            .projects
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(&params.0.project))
            .ok_or_else(|| {
                McpError::invalid_params(format!("Project '{}' not found", params.0.project), None)
            })?;

        // Check for duplicate service name
        if project.services.iter().any(|s| s.name.eq_ignore_ascii_case(&params.0.name)) {
            return Err(McpError::invalid_params(
                format!("Service '{}' already exists in project '{}'", params.0.name, project.name),
                None,
            ));
        }

        let target = match params.0.target.as_deref() {
            Some(t) if t.eq_ignore_ascii_case("wsl") => Target::Wsl,
            Some(t) if t.eq_ignore_ascii_case("docker") => Target::Docker,
            _ => Target::Windows,
        };

        let service = Service {
            name: params.0.name.clone(),
            command: params.0.command.clone(),
            target,
            working_dir: Some(params.0.working_dir.clone()),
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
        };

        let project_name = project.name.clone();
        project.services.push(service);

        save_global_config(&config)
            .map_err(|e| McpError::internal_error(format!("Failed to save config: {}", e), None))?;

        info!(
            project = %project_name,
            service = %params.0.name,
            "MCP: Service added"
        );

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Service '{}' added to project '{}' (target: {}, command: {})",
            params.0.name, project_name, target, params.0.command,
        ))]))
    }

    #[tool(description = "Save a technical debt snapshot for a project. Analyzes all services and stores metrics (LOC, anti-patterns, complexity, coverage) for tracking over time.")]
    async fn save_debt_snapshot(
        &self,
        params: Parameters<SaveDebtRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let mut analysis_results: Vec<(String, void_stack_core::analyzer::AnalysisResult)> = Vec::new();

        for svc in &project.services {
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let clean = strip_win_prefix(dir);
            let path = std::path::Path::new(&clean);
            if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
                analysis_results.push((svc.name.clone(), result));
            }
        }

        if analysis_results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No analyzable code found in any service. Snapshot not created.".to_string()
            )]));
        }

        let snapshot = void_stack_core::analyzer::history::create_snapshot(
            &analysis_results,
            params.0.label.clone(),
        );

        let root = strip_win_prefix(&project.path);
        let root_path = std::path::Path::new(&root);
        void_stack_core::analyzer::history::save_snapshot(root_path, &snapshot)
            .map_err(|e| McpError::internal_error(format!("Failed to save snapshot: {}", e), None))?;

        let label_str = params.0.label.as_deref().unwrap_or("(unlabeled)");
        let svc_count = snapshot.services.len();
        let total_loc: usize = snapshot.services.iter().map(|s| s.total_loc).sum();
        let total_ap: usize = snapshot.services.iter().map(|s| s.anti_pattern_count).sum();

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Snapshot saved for '{}' (label: {})\n  Services analyzed: {}\n  Total LOC: {}\n  Total anti-patterns: {}\n  Timestamp: {}",
            project.name, label_str, svc_count, total_loc, total_ap, snapshot.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
        ))]))
    }

    #[tool(description = "List all saved technical debt snapshots for a project, showing timestamp, label, and summary metrics.")]
    async fn list_debt_snapshots(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let root = strip_win_prefix(&project.path);
        let snapshots = void_stack_core::analyzer::history::load_snapshots(std::path::Path::new(&root));

        if snapshots.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No debt snapshots found for '{}'. Use save_debt_snapshot to create one.", project.name,
            ))]));
        }

        let mut lines = vec![format!("Debt snapshots for '{}' ({} total):\n", project.name, snapshots.len())];
        for (i, snap) in snapshots.iter().enumerate() {
            let label = snap.label.as_deref().unwrap_or("-");
            let total_loc: usize = snap.services.iter().map(|s| s.total_loc).sum();
            let total_ap: usize = snap.services.iter().map(|s| s.anti_pattern_count).sum();
            lines.push(format!(
                "  [{}] {} | label: {} | services: {} | LOC: {} | anti-patterns: {}",
                i,
                snap.timestamp.format("%Y-%m-%d %H:%M"),
                label,
                snap.services.len(),
                total_loc,
                total_ap,
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(lines.join("\n"))]))
    }

    #[tool(description = "Compare two technical debt snapshots for a project. Defaults to comparing the last two snapshots. Returns a markdown table showing deltas in LOC, anti-patterns, complexity, and coverage.")]
    async fn compare_debt(
        &self,
        params: Parameters<CompareDebtRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let root = strip_win_prefix(&project.path);
        let snapshots = void_stack_core::analyzer::history::load_snapshots(std::path::Path::new(&root));

        if snapshots.len() < 2 {
            return Err(McpError::invalid_params(
                format!(
                    "Need at least 2 snapshots to compare. Project '{}' has {}.",
                    project.name, snapshots.len(),
                ),
                None,
            ));
        }

        let idx_a = params.0.index_a.unwrap_or(snapshots.len() - 2);
        let idx_b = params.0.index_b.unwrap_or(snapshots.len() - 1);

        if idx_a >= snapshots.len() || idx_b >= snapshots.len() {
            return Err(McpError::invalid_params(
                format!(
                    "Index out of range. Valid range: 0..{} (total: {} snapshots)",
                    snapshots.len() - 1, snapshots.len(),
                ),
                None,
            ));
        }

        let comparison = void_stack_core::analyzer::history::compare(&snapshots[idx_a], &snapshots[idx_b]);
        let markdown = void_stack_core::analyzer::history::comparison_markdown(&comparison);

        Ok(CallToolResult::success(vec![Content::text(markdown)]))
    }

    #[tool(description = "Detect cross-project coupling between all registered projects. Finds import dependencies that reference other registered VoidStack projects.")]
    async fn analyze_cross_project(&self) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;

        if config.projects.len() < 2 {
            return Ok(CallToolResult::success(vec![Content::text(
                "Need at least 2 registered projects to detect cross-project coupling.".to_string()
            )]));
        }

        // Analyze all projects
        let mut analysis_results: HashMap<String, Vec<(String, void_stack_core::analyzer::AnalysisResult)>> =
            HashMap::new();

        for project in &config.projects {
            let mut svc_results = Vec::new();
            for svc in &project.services {
                let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
                let clean = strip_win_prefix(dir);
                let path = std::path::Path::new(&clean);
                if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
                    svc_results.push((svc.name.clone(), result));
                }
            }
            if !svc_results.is_empty() {
                analysis_results.insert(project.name.clone(), svc_results);
            }
        }

        if analysis_results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No analyzable code found in any project.".to_string()
            )]));
        }

        let result = void_stack_core::analyzer::analyze_cross_project(&config.projects, &analysis_results);

        let mut output = String::new();
        output.push_str("## Cross-Project Coupling Analysis\n\n");

        if result.links.is_empty() {
            output.push_str("No cross-project dependencies detected.\n");
        } else {
            output.push_str(&format!("Found {} cross-project link(s):\n\n", result.links.len()));
            output.push_str("| From Project | Service | To Project | Via Dependency |\n");
            output.push_str("|-------------|---------|------------|----------------|\n");
            for link in &result.links {
                output.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    link.from_project, link.from_service, link.to_project, link.via_dependency,
                ));
            }
        }

        if !result.unmatched_external.is_empty() {
            let mut ext: Vec<_> = result.unmatched_external.iter().collect();
            ext.sort();
            let shown = ext.iter().take(30).map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
            output.push_str(&format!(
                "\n**External dependencies** (not matching any project): {} total\n{}{}\n",
                ext.len(),
                shown,
                if ext.len() > 30 { " ..." } else { "" },
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "Scan a project for reclaimable disk space (node_modules, venv, build artifacts, caches). Shows size and whether each item is safe to delete.")]
    async fn scan_project_space(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        let path = strip_win_prefix(&project.path);
        let entries = tokio::task::spawn_blocking(move || {
            void_stack_core::space::scan_project(std::path::Path::new(&path))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Scan failed: {}", e), None))?;

        if entries.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No heavy directories found in '{}'.", project.name,
            ))]));
        }

        let total: u64 = entries.iter().map(|e| e.size_bytes).sum();
        let total_human = format_size(total);

        let mut lines = vec![format!(
            "Disk space scan for '{}' — {} reclaimable ({}):\n",
            project.name, total_human, entries.len(),
        )];
        for entry in &entries {
            let deletable = if entry.deletable { "safe to delete" } else { "NOT safe" };
            lines.push(format!(
                "  - {} [{}] — {} ({})\n    path: {}\n    restore: {}",
                entry.name, entry.category, entry.size_human, deletable, entry.path, entry.restore_hint,
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(lines.join("\n"))]))
    }

    #[tool(description = "Scan global caches and AI model storage for reclaimable disk space (pip cache, npm cache, Ollama models, HuggingFace cache, etc.).")]
    async fn scan_global_space(&self) -> Result<CallToolResult, McpError> {
        let entries = tokio::task::spawn_blocking(|| {
            void_stack_core::space::scan_global()
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Scan failed: {}", e), None))?;

        if entries.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No global caches or model storage found.".to_string()
            )]));
        }

        let total: u64 = entries.iter().map(|e| e.size_bytes).sum();
        let total_human = format_size(total);

        let mut lines = vec![format!(
            "Global disk space scan — {} total ({} entries):\n",
            total_human, entries.len(),
        )];
        for entry in &entries {
            let deletable = if entry.deletable { "safe to delete" } else { "NOT safe" };
            lines.push(format!(
                "  - {} [{}] — {} ({})\n    path: {}\n    restore: {}",
                entry.name, entry.category, entry.size_human, deletable, entry.path, entry.restore_hint,
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(lines.join("\n"))]))
    }

    #[tool(description = "Analyze Docker artifacts in a project: parse existing Dockerfile and docker-compose.yml, showing services, ports, images, volumes, and healthchecks.")]
    async fn docker_analyze(&self, params: Parameters<ProjectName>) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let proj = Self::find_project_or_err(&config, &params.0.project)?;
        let clean = strip_win_prefix(&proj.path);
        let project_path = std::path::PathBuf::from(&clean);

        let analysis = tokio::task::spawn_blocking(move || {
            void_stack_core::docker::analyze_docker(&project_path)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Analysis failed: {}", e), None))?;

        let mut lines = Vec::new();
        lines.push(format!("Docker Analysis: {}\n", proj.name));

        if analysis.has_dockerfile {
            lines.push("Dockerfile: found".to_string());
            if let Some(ref df) = analysis.dockerfile {
                for (i, stage) in df.stages.iter().enumerate() {
                    let name = stage.name.as_deref().unwrap_or("(unnamed)");
                    lines.push(format!("  Stage {}: {} ({})", i, stage.base_image, name));
                }
                if !df.exposed_ports.is_empty() {
                    lines.push(format!("  Ports: {:?}", df.exposed_ports));
                }
            }
        } else {
            lines.push("Dockerfile: not found".to_string());
        }

        if analysis.has_compose {
            lines.push("docker-compose: found".to_string());
            if let Some(ref compose) = analysis.compose {
                for svc in &compose.services {
                    let img = svc.image.as_deref().unwrap_or("build");
                    let ports: Vec<String> = svc.ports.iter().map(|p| format!("{}:{}", p.host, p.container)).collect();
                    lines.push(format!("  {} ({}) → {} [{}]", svc.name, svc.kind, img, ports.join(", ")));
                }
            }
        } else {
            lines.push("docker-compose: not found".to_string());
        }

        // Terraform
        if !analysis.terraform.is_empty() {
            lines.push(format!("\nTerraform ({} resources):", analysis.terraform.len()));
            for res in &analysis.terraform {
                let details = if res.details.is_empty() { String::new() } else { format!(" ({})", res.details.join(", ")) };
                lines.push(format!("  [{}] {} \"{}\" → {}{}", res.provider, res.resource_type, res.name, res.kind, details));
            }
        }

        // Kubernetes
        if !analysis.kubernetes.is_empty() {
            lines.push(format!("\nKubernetes ({} resources):", analysis.kubernetes.len()));
            for res in &analysis.kubernetes {
                let ns = res.namespace.as_deref().unwrap_or("default");
                let images = if res.images.is_empty() { String::new() } else { format!(" images=[{}]", res.images.join(", ")) };
                lines.push(format!("  {}: {} (ns={}){}",
                    res.kind, res.name, ns, images));
            }
        }

        // Helm
        if let Some(ref chart) = analysis.helm {
            lines.push(format!("\nHelm: {} v{}", chart.name, chart.version));
            for dep in &chart.dependencies {
                lines.push(format!("  dep: {} ({}) → {}", dep.name, dep.version, dep.repository));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(lines.join("\n"))]))
    }

    #[tool(description = "Generate a Dockerfile and/or docker-compose.yml for a project based on detected frameworks and dependencies. Optionally saves files to the project directory.")]
    async fn docker_generate(&self, params: Parameters<DockerGenerateRequest>) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let proj = Self::find_project_or_err(&config, &params.0.project)?;
        let clean = strip_win_prefix(&proj.path);
        let project_path = std::path::PathBuf::from(&clean);
        let gen_df = params.0.generate_dockerfile.unwrap_or(true);
        let gen_compose = params.0.generate_compose.unwrap_or(true);
        let save = params.0.save.unwrap_or(false);

        let proj_clone = proj.clone();
        let path_clone = project_path.clone();

        let result = tokio::task::spawn_blocking(move || {
            let mut dockerfile_content = None;
            let mut compose_content = None;
            let mut saved = Vec::new();

            if gen_df && !path_clone.join("Dockerfile").exists() {
                let pt = void_stack_core::config::detect_project_type(&path_clone);
                if let Some(content) = void_stack_core::docker::generate_dockerfile::generate(&path_clone, pt) {
                    if save {
                        let out = path_clone.join("Dockerfile");
                        let _ = std::fs::write(&out, &content);
                        saved.push(out.to_string_lossy().to_string());
                    }
                    dockerfile_content = Some(content);
                }
            }

            if gen_compose {
                let content = void_stack_core::docker::generate_compose::generate(&proj_clone, &path_clone);
                if save {
                    let out = path_clone.join("docker-compose.yml");
                    let _ = std::fs::write(&out, &content);
                    saved.push(out.to_string_lossy().to_string());
                }
                compose_content = Some(content);
            }

            (dockerfile_content, compose_content, saved)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Generation failed: {}", e), None))?;

        let mut output = Vec::new();

        if let Some(ref df) = result.0 {
            output.push(format!("── Generated Dockerfile ──\n\n{}", df));
        }
        if let Some(ref compose) = result.1 {
            output.push(format!("── Generated docker-compose.yml ──\n\n{}", compose));
        }
        if !result.2.is_empty() {
            output.push(format!("Saved to:\n{}", result.2.join("\n")));
        }

        if output.is_empty() {
            output.push("No files generated (Dockerfile already exists or unsupported project type).".to_string());
        }

        Ok(CallToolResult::success(vec![Content::text(output.join("\n\n"))]))
    }

    #[tool(description = "Generate AI-powered refactoring suggestions for a project using Ollama (local LLM). Analyzes code architecture, anti-patterns, complexity, and coverage, then asks an LLM for actionable improvement suggestions. If Ollama is not available, returns the analysis context for you to reason about directly.")]
    async fn suggest_refactoring(
        &self,
        params: Parameters<SuggestRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;

        // Find first analyzable service directory
        let services: Vec<_> = match &params.0.service {
            Some(svc_name) => {
                project.services.iter()
                    .filter(|s| s.name.eq_ignore_ascii_case(svc_name))
                    .collect()
            }
            None => project.services.iter().collect(),
        };

        let mut analysis = None;
        for svc in &services {
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let clean = strip_win_prefix(dir);
            let path = std::path::Path::new(&clean);
            if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
                analysis = Some(result);
                break;
            }
        }

        let analysis = match analysis {
            Some(a) => a,
            None => {
                return Ok(CallToolResult::success(vec![Content::text(
                    "No analyzable code found in the project (supported: Python, JavaScript/TypeScript, Go, Rust, Dart).".to_string()
                )]));
            }
        };

        // Load AI config
        let mut ai_config = void_stack_core::ai::load_ai_config().unwrap_or_default();
        if let Some(model) = &params.0.model {
            ai_config.model = model.clone();
        }

        // Try to call Ollama; if unavailable, return the analysis context
        match void_stack_core::ai::suggest(&ai_config, &analysis, &project.name).await {
            Ok(result) => {
                let mut output = format!("## Sugerencias de AI (modelo: {})\n\n", result.model_used);
                for (i, s) in result.suggestions.iter().enumerate() {
                    output.push_str(&format!(
                        "### {}. [{}] {} ({})\n{}\n",
                        i + 1, s.category, s.title, s.priority,
                        s.description,
                    ));
                    if !s.affected_files.is_empty() {
                        output.push_str(&format!("Archivos: {}\n", s.affected_files.join(", ")));
                    }
                    output.push('\n');
                }
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            Err(_) => {
                // Fallback: return analysis context for the AI assistant to process directly
                let context = void_stack_core::ai::build_context(&analysis, &project.name);
                let output = format!(
                    "Ollama no está disponible. Aquí está el contexto de análisis para que generes sugerencias directamente:\n\n{}",
                    context,
                );
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
        }
    }
}

/// Format bytes into human-readable size.
fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
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
impl ServerHandler for VoidStackMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "VoidStack MCP server — manage development service projects. \
                 Use list_projects to see registered projects, start_project/stop_project \
                 to manage services, get_logs for output, and add_project to register new ones.",
            )
    }
}
