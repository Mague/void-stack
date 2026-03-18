use std::collections::HashMap;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use void_stack_core::global_config::{GlobalConfig, find_project, load_global_config};
use void_stack_core::manager::ProcessManager;
use void_stack_core::model::Project;

use crate::tools;

// ── Tool parameter types ────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ProjectName {
    /// Name of the project (case-insensitive)
    pub project: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ServiceRef {
    /// Name of the project
    pub project: String,
    /// Name of the service within the project
    pub service: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct LogsRequest {
    /// Name of the project
    pub project: String,
    /// Name of the service
    pub service: String,
    /// Maximum number of log lines to return (default: 50)
    #[serde(default = "default_log_lines")]
    pub lines: usize,
}

fn default_log_lines() -> usize {
    50
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct DiagramRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Output format: "mermaid" or "drawio" (default: drawio)
    pub format: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AnalyzeRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Specific service to analyze (omit for all services)
    pub service: Option<String>,
    /// Include best practices analysis (ruff, clippy, golangci-lint, react-doctor, dart analyze)
    pub best_practices: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddProjectRequest {
    /// Name for the project
    pub name: String,
    /// Absolute path to the project directory (Windows path or WSL UNC path like \\\\wsl.localhost\\Ubuntu\\home\\user\\project)
    pub path: String,
    /// Set to true if the project is inside WSL. When true, provide a Linux path and specify the distro.
    #[serde(default)]
    pub wsl: bool,
    /// WSL distro name (e.g., "Ubuntu"). Required when wsl=true and path is a Linux path.
    pub distro: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadDocsRequest {
    /// Name of the project
    pub project: String,
    /// Filename to read (default: README.md). Supports: README.md, CHANGELOG.md, CLAUDE.md, etc.
    #[serde(default = "default_doc_file")]
    pub filename: String,
}

fn default_doc_file() -> String {
    "README.md".to_string()
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadFileRequest {
    /// Name of the project
    pub project: String,
    /// Relative path to the file within the project (e.g. "src/main.rs", "Cargo.toml", "diagram.drawio")
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ScanDirectoryRequest {
    /// Absolute path to the directory to scan
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddServiceRequest {
    /// Name of the project to add the service to (case-insensitive)
    pub project: String,
    /// Name for the new service
    pub name: String,
    /// Command to run the service (e.g., "npm run dev") or Docker image (e.g., "postgres:16")
    pub command: String,
    /// Absolute path to the service's working directory
    pub working_dir: String,
    /// Execution target: "windows", "wsl", or "docker" (default: windows)
    pub target: Option<String>,
    /// Docker port mappings (e.g., ["5432:5432", "8080:80"]). Only used when target = "docker".
    pub docker_ports: Option<Vec<String>>,
    /// Docker volume mounts (e.g., ["./data:/var/lib/data"]). Only used when target = "docker".
    pub docker_volumes: Option<Vec<String>>,
    /// Extra docker run arguments (e.g., ["--network=host"]). Only used when target = "docker".
    pub docker_extra_args: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SaveDebtRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Optional label for the snapshot (e.g., git tag or version)
    pub label: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct CompareDebtRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Index of the first snapshot to compare (0-based). Defaults to second-to-last.
    pub index_a: Option<usize>,
    /// Index of the second snapshot to compare (0-based). Defaults to last.
    pub index_b: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct DockerGenerateRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Generate a Dockerfile if one doesn't exist (default: true)
    pub generate_dockerfile: Option<bool>,
    /// Generate a docker-compose.yml (default: true)
    pub generate_compose: Option<bool>,
    /// Save generated files to the project directory (default: false)
    pub save: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SuggestRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Override model (e.g., "llama3.2", "qwen2.5-coder:7b"). Uses config default if omitted.
    pub model: Option<String>,
    /// Specific service to analyze (omit for first analyzable service)
    pub service: Option<String>,
}

// ── Response types ──────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct ProjectInfo {
    pub name: String,
    pub path: String,
    pub project_type: String,
    pub services: Vec<ServiceInfo>,
}

#[derive(Serialize)]
pub(crate) struct ServiceInfo {
    pub name: String,
    pub command: String,
    pub target: String,
    pub working_dir: Option<String>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_ports: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_volumes: Option<Vec<String>>,
}

#[derive(Serialize)]
pub(crate) struct ServiceStateInfo {
    pub name: String,
    pub status: String,
    pub pid: Option<u32>,
    pub url: Option<String>,
    pub last_log: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct StartStopResult {
    pub project: String,
    pub results: Vec<ServiceStateInfo>,
}

// ── MCP Server ──────────────────────────────────────────────

#[derive(Clone)]
pub struct VoidStackMcp {
    /// Active ProcessManagers keyed by project name
    pub(crate) managers: Arc<Mutex<HashMap<String, Arc<ProcessManager>>>>,
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
    pub(crate) async fn get_manager(&self, project: &Project) -> Arc<ProcessManager> {
        let mut managers = self.managers.lock().await;
        if let Some(mgr) = managers.get(&project.name) {
            return Arc::clone(mgr);
        }
        let mgr = Arc::new(ProcessManager::new(project.clone()));
        managers.insert(project.name.clone(), Arc::clone(&mgr));
        mgr
    }

    pub(crate) fn load_config() -> Result<GlobalConfig, McpError> {
        load_global_config()
            .map_err(|e| McpError::internal_error(format!("Failed to load config: {}", e), None))
    }

    pub(crate) fn find_project_or_err(
        config: &GlobalConfig,
        name: &str,
    ) -> Result<Project, McpError> {
        find_project(config, name)
            .cloned()
            .ok_or_else(|| McpError::invalid_params(format!("Project '{}' not found", name), None))
    }

    // ── Tools ───────────────────────────────────────────────

    #[tool(description = "List all registered VoidStack projects with their services")]
    async fn list_projects(&self) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        tools::projects::list_projects(&config)
    }

    #[tool(
        description = "Get the live status of all services in a project (running, stopped, PIDs, URLs)"
    )]
    async fn project_status(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::services::project_status(self, &project).await
    }

    #[tool(
        description = "Start all services in a project. Returns immediately. Use project_status afterwards to get detected URLs."
    )]
    async fn start_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::services::start_project(self, &project).await
    }

    #[tool(description = "Stop all services in a project")]
    async fn stop_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::services::stop_project(self, &project).await
    }

    #[tool(
        description = "Start a specific service within a project. Use project_status afterwards to get the detected URL."
    )]
    async fn start_service(
        &self,
        params: Parameters<ServiceRef>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::services::start_service(self, &project, &params.0.service).await
    }

    #[tool(description = "Stop a specific service within a project")]
    async fn stop_service(
        &self,
        params: Parameters<ServiceRef>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::services::stop_service(self, &project, &params.0.service).await
    }

    #[tool(description = "Get recent log output from a service")]
    async fn get_logs(&self, params: Parameters<LogsRequest>) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::services::get_logs(self, &project, &params.0.service, params.0.lines).await
    }

    #[tool(
        description = "Scan a directory and register it as a VoidStack project with auto-detected services. For WSL projects, set wsl=true and provide distro name."
    )]
    async fn add_project(
        &self,
        params: Parameters<AddProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::add_project(
            &params.0.name,
            &params.0.path,
            params.0.wsl,
            params.0.distro.as_deref(),
        )
    }

    #[tool(
        description = "Read documentation files (README.md, CHANGELOG.md, CLAUDE.md, etc.) from a project directory to understand what the project does"
    )]
    async fn read_project_docs(
        &self,
        params: Parameters<ReadDocsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::docs::read_project_docs(&project, &params.0.filename)
    }

    #[tool(
        description = "Read ALL documentation files from a project at once (README.md, CHANGELOG.md, CLAUDE.md, etc.). Returns all found doc files concatenated. Use this at the start of a conversation to quickly understand a project."
    )]
    async fn read_all_docs(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::docs::read_all_docs(&project)
    }

    #[tool(
        description = "Read any file from a registered project by relative path. Use this after generate_diagram to read the saved .drawio file, or to read .proto files, Cargo.toml, pubspec.yaml, or any project file. Blocked: .env, credentials, private keys. Max 200KB (truncated with warning if larger)."
    )]
    async fn read_project_file(
        &self,
        params: Parameters<ReadFileRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::docs::read_project_file(&project, &params.0.path)
    }

    #[tool(
        description = "List all files in a registered project (up to 3 levels deep). Excludes sensitive files, node_modules, target, .git, and other build directories."
    )]
    async fn list_project_files(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::docs::list_project_files_tool(&project)
    }

    #[tool(
        description = "Generate architecture, API routes, and DB model diagrams for a project. Supports 'mermaid' (returns markdown) and 'drawio' (saves .drawio file to project dir and returns path). Default format is drawio."
    )]
    async fn generate_diagram(
        &self,
        params: Parameters<DiagramRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::diagrams::generate_diagram(&project, params.0.format.as_deref())
    }

    #[tool(
        description = "Check all dependencies for a project (Python, Node, CUDA, Ollama, Docker, .env). Returns status, versions, and fix hints for each dependency."
    )]
    async fn check_dependencies(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::analysis::check_dependencies(&project).await
    }

    #[tool(
        description = "Analyze code architecture: dependency graph, architecture patterns (MVC, Layered, Clean, Monolith), anti-patterns (god class, circular deps, fat controllers, excessive coupling). Returns markdown documentation. Optionally specify a service name to analyze a single service."
    )]
    async fn analyze_project(
        &self,
        params: Parameters<AnalyzeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::analysis::analyze_project(
            &project,
            params.0.service.as_deref(),
            params.0.best_practices.unwrap_or(false),
        )
    }

    #[tool(
        description = "Run security audit on a project: scan for vulnerable dependencies (npm audit, pip audit, cargo audit), hardcoded secrets (API keys, tokens, passwords), and insecure configurations (debug mode, open CORS, Docker issues). Returns findings with severity, description, and remediation steps."
    )]
    async fn audit_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::analysis::audit_project(&project)
    }

    #[tool(description = "Remove a registered project from VoidStack")]
    async fn remove_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::remove_project_tool(self, &params.0.project).await
    }

    #[tool(
        description = "Preview what services would be detected at a directory path, without registering the project. Useful for checking before adding."
    )]
    async fn scan_directory(
        &self,
        params: Parameters<ScanDirectoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::scan_directory(&params.0.path)
    }

    #[tool(
        description = "Add a service to an existing registered project. Specify the command, working directory, and optionally the target (windows/wsl/docker)."
    )]
    async fn add_service(
        &self,
        params: Parameters<AddServiceRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::add_service(&params.0)
    }

    #[tool(
        description = "Save a technical debt snapshot for a project. Analyzes all services and stores metrics (LOC, anti-patterns, complexity, coverage) for tracking over time."
    )]
    async fn save_debt_snapshot(
        &self,
        params: Parameters<SaveDebtRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::debt::save_debt_snapshot(&project, params.0.label.as_deref())
    }

    #[tool(
        description = "List all saved technical debt snapshots for a project, showing timestamp, label, and summary metrics."
    )]
    async fn list_debt_snapshots(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::debt::list_debt_snapshots(&project)
    }

    #[tool(
        description = "Compare two technical debt snapshots for a project. Defaults to comparing the last two snapshots. Returns a markdown table showing deltas in LOC, anti-patterns, complexity, and coverage."
    )]
    async fn compare_debt(
        &self,
        params: Parameters<CompareDebtRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::debt::compare_debt(&project, params.0.index_a, params.0.index_b)
    }

    #[tool(
        description = "Detect cross-project coupling between all registered projects. Finds import dependencies that reference other registered VoidStack projects."
    )]
    async fn analyze_cross_project(&self) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        tools::analysis::analyze_cross_project(&config)
    }

    #[tool(
        description = "Scan a project for reclaimable disk space (node_modules, venv, build artifacts, caches). Shows size and whether each item is safe to delete."
    )]
    async fn scan_project_space(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::space::scan_project_space(&project).await
    }

    #[tool(
        description = "Scan global caches and AI model storage for reclaimable disk space (pip cache, npm cache, Ollama models, HuggingFace cache, etc.)."
    )]
    async fn scan_global_space(&self) -> Result<CallToolResult, McpError> {
        tools::space::scan_global_space().await
    }

    #[tool(
        description = "Analyze Docker artifacts in a project: parse existing Dockerfile and docker-compose.yml, showing services, ports, images, volumes, and healthchecks."
    )]
    async fn docker_analyze(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::docker::docker_analyze(&project).await
    }

    #[tool(
        description = "Generate a Dockerfile and/or docker-compose.yml for a project based on detected frameworks and dependencies. Optionally saves files to the project directory."
    )]
    async fn docker_generate(
        &self,
        params: Parameters<DockerGenerateRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::docker::docker_generate(
            &project,
            params.0.generate_dockerfile.unwrap_or(true),
            params.0.generate_compose.unwrap_or(true),
            params.0.save.unwrap_or(false),
        )
        .await
    }

    #[tool(
        description = "Generate AI-powered refactoring suggestions for a project using Ollama (local LLM). Analyzes code architecture, anti-patterns, complexity, and coverage, then asks an LLM for actionable improvement suggestions. If Ollama is not available, returns the analysis context for you to reason about directly."
    )]
    async fn suggest_refactoring(
        &self,
        params: Parameters<SuggestRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::suggest::suggest_refactoring(
            &project,
            params.0.service.as_deref(),
            params.0.model.as_deref(),
        )
        .await
    }
}

// ── ServerHandler ───────────────────────────────────────────

#[tool_handler]
impl ServerHandler for VoidStackMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "VoidStack MCP server — manage development service projects. \
                 Use list_projects to see registered projects, start_project/stop_project \
                 to manage services, get_logs for output, and add_project to register new ones.",
        )
    }
}
