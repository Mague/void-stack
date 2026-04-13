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
    /// Set to true to get raw unfiltered output (default: false, auto-filters noise)
    #[serde(default)]
    pub raw: bool,
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
    /// Compact output (~90% smaller): only summary, critical anti-patterns, top-5 complex functions, coverage. Ideal for initial triage before deep-diving.
    pub compact: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AuditRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Verbose output: full details for all severities (default: false = compact, details only for Critical/High)
    pub verbose: Option<bool>,
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
pub(crate) struct GenerateClaudeIgnoreRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// If true, return the generated content without saving to disk
    pub dry_run: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct IndexProjectRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Force re-index all files (default: false, incremental)
    #[serde(default)]
    pub force: bool,
    /// Git ref to diff against for change detection (e.g. "HEAD", "HEAD~1", "main").
    /// When provided, only files changed since this ref are re-indexed — faster
    /// and more accurate than mtime comparison after checkout/stash/pull.
    #[serde(default)]
    pub git_base: Option<String>,
}

#[cfg(feature = "structural")]
#[derive(Deserialize, JsonSchema)]
pub(crate) struct StructuralBuildRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Force re-parse of every file (default: false, incremental by SHA-256)
    #[serde(default)]
    pub force: bool,
}

#[cfg(feature = "structural")]
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ImpactRadiusRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Files to treat as changed (relative paths). Omit to auto-detect via git diff HEAD~1.
    #[serde(default)]
    pub changed_files: Option<Vec<String>>,
    /// BFS depth limit (default: 2). Increase to reach transitive dependencies.
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// Restrict traversal to CALLS edges (default: true). Set false to include
    /// IMPORTS_FROM edges — much slower on TypeScript/JavaScript projects
    /// where imports can fan out to thousands of neighbours per node.
    #[serde(default)]
    pub only_calls: Option<bool>,
}

#[cfg(feature = "structural")]
#[derive(Deserialize, JsonSchema)]
pub(crate) struct QueryGraphRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// qualified_name (file::ClassName::method) or bare name for search
    pub target: String,
    /// One of: "callers", "callees", "tests", "search"
    pub query_type: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SemanticSearchRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Natural language query (e.g. "authentication middleware", "database connection pool")
    pub query: String,
    /// Number of results to return (default: 5)
    pub top_k: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct TokenStatsRequest {
    /// Filter by project name (omit for all projects)
    pub project: Option<String>,
    /// Number of days to include (default: 30)
    pub days: Option<u32>,
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
    #[allow(dead_code)]
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

    #[tool(
        description = "Get recent log output from a service. Call project_status first to see which services are running and their names before fetching logs."
    )]
    async fn get_logs(&self, params: Parameters<LogsRequest>) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::services::get_logs(
            self,
            &project,
            &params.0.service,
            params.0.lines,
            params.0.raw,
        )
        .await
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
        description = "Read project documentation (README, CHANGELOG, CLAUDE.md) from disk. Call this at session start for PROJECT CONTEXT (architecture, setup, decisions). NOT for code understanding — use semantic_search for that. Workflow: (1) get_index_stats to check if index exists, (2a) if YES: semantic_search for code questions + read_all_docs for project context, (2b) if NO: read_all_docs first, then index_project_codebase for future sessions."
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
        description = "FALLBACK: prefer semantic_search when index exists (check with get_index_stats). Use read_project_file only when you need a SPECIFIC file by exact path and semantic_search didn't return enough context. Read any file from a registered project by relative path. Also useful after generate_diagram to read the saved .drawio file, or to read .proto files, Cargo.toml, pubspec.yaml. Blocked: .env, credentials, private keys. Max 200KB (truncated with warning if larger)."
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
        description = "Generate architecture, API routes, and DB model diagrams. For drawio format: saves .drawio file to project dir AND returns the full XML content so you can inspect it directly. For mermaid: returns markdown. Call read_all_docs first to have project context before generating diagrams."
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
        description = "Check all dependencies for a project (Python, Node, CUDA, Ollama, Docker, .env). Returns status, versions, and fix hints for each dependency. Call this before start_project if services are failing to detect missing dependencies."
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
        description = "Analyze code architecture: dependency graph, architecture patterns (MVC, Layered, Clean, Monolith), anti-patterns (god class, circular deps, fat controllers, excessive coupling). Returns markdown documentation. Call read_all_docs first if you haven't loaded project context yet. Save results with save_debt_snapshot to track trends over time."
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
            params.0.compact.unwrap_or(false),
        )
    }

    #[tool(
        description = "Run security audit on a project: scan for vulnerable dependencies (npm audit, pip audit, cargo audit), hardcoded secrets (API keys, tokens, passwords), and insecure configurations (debug mode, open CORS, Docker issues). Default: compact output (full detail for Critical/High, title-only for Medium, count for Low/Info). Set verbose=true for full details on all severities."
    )]
    async fn audit_project(
        &self,
        params: Parameters<AuditRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::analysis::audit_project(&project, params.0.verbose.unwrap_or(false))
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
        description = "Save a technical debt snapshot for a project. Call analyze_project first to get fresh metrics. Use --label to tag releases (e.g. 'v1.0'). Compare with compare_debt to track trends over time."
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
        description = "Compare two technical debt snapshots for a project. Requires at least 2 previous save_debt_snapshot calls. Use list_debt_snapshots to see available snapshots first. Defaults to comparing the last two. Returns a markdown table showing deltas in LOC, anti-patterns, complexity, and coverage."
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
        description = "Generate AI-powered refactoring suggestions for a project using Ollama (local LLM). Runs analyze_project internally — no need to call it first. Requires Ollama running locally (check with check_dependencies). If Ollama is not available, returns the analysis context for you to reason about directly."
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

    #[tool(
        description = "Generate a .claudeignore file for a project based on its detected tech stack (Rust, Go, Flutter, Node, Python). Reduces Claude Code token consumption by excluding generated files, build artifacts, and dependencies from the agent's context. Returns the generated content and saves it to the project root."
    )]
    async fn generate_claudeignore(
        &self,
        params: Parameters<GenerateClaudeIgnoreRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::docs::generate_claudeignore_tool(&project, params.0.dry_run.unwrap_or(false))
    }

    #[tool(
        description = "Get token savings statistics for Void Stack operations (log filtering, claudeignore generation). Shows how many tokens have been saved by using Void Stack's optimization features. Useful for tracking efficiency over time."
    )]
    async fn get_token_stats(
        &self,
        params: Parameters<TokenStatsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::stats::get_token_stats(params.0.project.as_deref(), params.0.days.unwrap_or(30))
    }

    #[tool(
        description = "Run once per project before using semantic_search. Non-blocking: returns immediately, builds index in background (~30-120s depending on project size). Call get_index_stats to monitor progress. Re-run incrementally after significant code changes (only modified files are re-indexed, fast). Uses BAAI/bge-small-en-v1.5 embeddings (runs 100% locally, no API key, ~130MB one-time download). Respects .claudeignore and .voidignore to skip generated files and build artifacts."
    )]
    async fn index_project_codebase(
        &self,
        params: Parameters<IndexProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::search::index_project_codebase(&project, params.0.force, params.0.git_base)
    }

    #[tool(
        description = "PRIMARY tool for understanding code. Search the indexed codebase with natural language — returns only relevant chunks, 40-60% fewer tokens than reading files. Use for: finding implementations, understanding logic, locating bugs, exploring architecture. Requires index to exist (check with get_index_stats first). ALWAYS prefer over read_project_file and read_all_docs for code questions. Examples: 'handlePublish marketplace logic', 'authentication middleware flow', 'database connection pool'."
    )]
    async fn semantic_search(
        &self,
        params: Parameters<SemanticSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::search::semantic_search(&project, &params.0.query, params.0.top_k.unwrap_or(5))
    }

    #[tool(
        description = "START HERE. Call this first in any session to check if a semantic index exists. If it returns stats (files_indexed, created_at): use semantic_search for code questions — faster and 40-60% fewer tokens. If it returns 'No index found': call index_project_codebase to build one (runs in background, non-blocking), then read_all_docs while waiting."
    )]
    async fn get_index_stats(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::search::get_index_stats(&project)
    }

    #[tool(
        description = "Start watching a project directory. File changes trigger an incremental semantic re-index automatically (~500ms debounce). Use this while actively editing code to keep the index fresh without manual runs. Call unwatch_project to stop."
    )]
    async fn watch_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::search::watch_project_tool(&project)
    }

    #[tool(
        description = "Stop watching a project previously started with watch_project. Idempotent — safe to call on an unwatched project."
    )]
    async fn unwatch_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::search::unwatch_project_tool(&project)
    }

    #[tool(
        description = "Install a git post-commit hook that triggers incremental re-indexing after each commit. Idempotent — running twice does not duplicate the hook entry. Requires the project to be a git repository."
    )]
    async fn install_index_hook(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::search::install_index_hook(&project)
    }

    #[cfg(feature = "structural")]
    #[tool(
        description = "Build (or incrementally update) a tree-sitter powered structural call graph for a project, stored at .void-stack/structural.db. Skips files whose SHA-256 matches the cached one unless force=true. Required before get_impact_radius or query_graph. Supports Rust, Python, JavaScript, TypeScript, Go."
    )]
    async fn build_structural_graph(
        &self,
        params: Parameters<StructuralBuildRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::structural::build_structural_graph_tool(&project, params.0.force)
    }

    #[cfg(feature = "structural")]
    #[tool(
        description = "Compute the blast radius of a set of changed files using the structural graph. Returns every function/class/file transitively affected up to max_depth. When changed_files is omitted, auto-detects them via git diff HEAD~1. only_calls=true (default) traverses CALLS edges only — much faster on TypeScript/JavaScript projects. Set only_calls=false to include IMPORTS_FROM edges too. Query is capped at 30s; on timeout the tool returns a hint instead of hanging. Requires build_structural_graph to have been run first."
    )]
    async fn get_impact_radius(
        &self,
        params: Parameters<ImpactRadiusRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::structural::get_impact_radius_tool(
            &project,
            params.0.changed_files,
            params.0.max_depth,
            params.0.only_calls,
        )
    }

    #[cfg(feature = "structural")]
    #[tool(
        description = "Query the structural call graph. query_type: 'callers' (who calls target), 'callees' (what target calls), 'tests' (tests that exercise target), 'search' (fuzzy find nodes by name). target is a qualified_name (file::ClassName::method) or a bare name for search."
    )]
    async fn query_graph(
        &self,
        params: Parameters<QueryGraphRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::structural::query_graph_tool(&project, params.0.target, params.0.query_type)
    }

    #[tool(
        description = "Generate an optimized .voidignore file for the project's semantic vector index. Excludes generated code, build artifacts, test fixtures, and files that don't carry business-logic semantics. Detects project tech stack (Rust, Node, Go, Python, Flutter) for stack-specific patterns. Run before index_project_codebase to improve index quality."
    )]
    async fn generate_voidignore(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::search::generate_voidignore(&project)
    }
}

// ── ServerHandler ───────────────────────────────────────────

#[tool_handler]
impl ServerHandler for VoidStackMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "VoidStack MCP server — unified development stack manager. \
                 RECOMMENDED WORKFLOW: (1) get_index_stats — check if semantic index exists. \
                 (2a) IF index exists: semantic_search for ANY code question (implementations, bugs, logic), \
                 read_all_docs for project context (README, architecture decisions), \
                 analyze_project for metrics and anti-patterns. \
                 (2b) IF no index: read_all_docs for immediate context, \
                 index_project_codebase to build index (background, non-blocking), \
                 semantic_search once index is ready. \
                 For code understanding: ALWAYS prefer semantic_search over read_project_file or read_all_docs. \
                 read_all_docs is for DOCUMENTATION, not for CODE understanding. \
                 For services: start_project → project_status → get_logs. \
                 For debt tracking: analyze_project → save_debt_snapshot → compare_debt.",
        )
    }
}
