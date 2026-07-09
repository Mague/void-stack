use std::collections::HashMap;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use tokio::sync::Mutex;

use void_stack_core::global_config::{GlobalConfig, find_project, load_global_config};
use void_stack_core::manager::ProcessManager;
use void_stack_core::model::Project;

use crate::tools;
use crate::types::*;

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

    /// Evict the cached ProcessManager for a project. Called after
    /// add_service / remove_service so the next get_manager() rebuilds
    /// from the freshly-saved config instead of returning a stale snapshot
    /// — without this, services added via MCP didn't appear in
    /// project_status until the MCP server was restarted.
    pub(crate) async fn invalidate_manager(&self, project_name: &str) {
        let mut managers = self.managers.lock().await;
        managers.remove(project_name);
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
        description = "Run security audit on a project: scan for vulnerable dependencies (npm audit, pip audit, cargo audit), hardcoded secrets (API keys, tokens, passwords), and insecure configurations (debug mode, open CORS, Docker issues). Default: compact output (full detail for Critical/High, title-only for Medium, count for Low/Info). Set verbose=true for full details on all severities. Related: review_diff runs these rules scoped to the changed lines only — prefer it before commits."
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
        description = "Rename and/or move a registered project WITHOUT losing derived data: the semantic index, structural graph, contracts cache, trust approval and git post-commit hook are migrated, never rebuilt. To move: relocate the directory yourself first, then call this with new_path."
    )]
    async fn update_project(
        &self,
        params: Parameters<UpdateProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::update_project_tool(
            &params.0.project,
            params.0.new_name.as_deref(),
            params.0.new_path.as_deref(),
        )
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
        let result = tools::projects::add_service(&params.0)?;
        // Drop any cached ProcessManager for this project so the next
        // project_status sees the new service immediately (Bug 2).
        self.invalidate_manager(&params.0.project).await;
        Ok(result)
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
        tools::search::index_project_codebase(self, params.0).await
    }

    #[tool(
        description = "PRIMARY tool for understanding code. Search the indexed codebase with natural language — returns only relevant chunks, 40-60% fewer tokens than reading files. Use for: finding implementations, understanding logic, locating bugs, exploring architecture. Requires index to exist (check with get_index_stats first). ALWAYS prefer over read_project_file and read_all_docs for code questions. Examples: 'handlePublish marketplace logic', 'authentication middleware flow', 'database connection pool'."
    )]
    async fn semantic_search(
        &self,
        params: Parameters<SemanticSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::semantic_search(self, params.0).await
    }

    #[tool(
        description = "START HERE. Call this first in any session to check if a semantic index exists. If it returns stats (files_indexed, created_at): use semantic_search for code questions — faster and 40-60% fewer tokens. If it returns 'No index found': call index_project_codebase to build one (runs in background, non-blocking), then read_all_docs while waiting."
    )]
    async fn get_index_stats(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::get_index_stats(self, params.0).await
    }

    #[tool(
        description = "Start watching a project directory. File changes trigger an incremental semantic re-index automatically (~500ms debounce). Use this while actively editing code to keep the index fresh without manual runs. Call unwatch_project to stop."
    )]
    async fn watch_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::watch_project(self, params.0).await
    }

    #[tool(
        description = "Stop watching a project previously started with watch_project. Idempotent — safe to call on an unwatched project."
    )]
    async fn unwatch_project(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::unwatch_project(self, params.0).await
    }

    #[tool(
        description = "Install a git post-commit hook that triggers incremental re-indexing after each commit. Idempotent — running twice does not duplicate the hook entry. Requires the project to be a git repository."
    )]
    async fn install_index_hook(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::install_index_hook(self, params.0).await
    }

    #[cfg(feature = "structural")]
    #[tool(
        description = "Build (or incrementally update) a tree-sitter powered structural call graph for a project, stored at .void-stack/structural.db. Skips files whose SHA-256 matches the cached one unless force=true. Required before get_impact_radius or query_graph. Supports Rust, Python, JavaScript, TypeScript, Go."
    )]
    async fn build_structural_graph(
        &self,
        params: Parameters<StructuralBuildRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::build_structural_graph(self, params.0).await
    }

    #[cfg(feature = "structural")]
    #[tool(
        description = "Compute the blast radius of a set of changed files using the structural graph. Returns every function/class/file transitively affected up to max_depth. When changed_files is omitted, auto-detects them via git diff HEAD~1. only_calls=true (default) traverses CALLS edges only — much faster on TypeScript/JavaScript projects. Set only_calls=false to include IMPORTS_FROM edges too. Query is capped at 30s; on timeout the tool returns a hint instead of hanging. Requires build_structural_graph to have been run first. Related: review_diff embeds a depth-2 blast radius for the current git diff; suggest_tests_for_diff tells you which tests cover it."
    )]
    async fn get_impact_radius(
        &self,
        params: Parameters<ImpactRadiusRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::get_impact_radius(self, params.0).await
    }

    #[cfg(feature = "structural")]
    #[tool(
        description = "Query the structural call graph. query_type: 'callers' (who calls target), 'callees' (what target calls), 'tests' (tests that exercise target), 'search' (fuzzy find nodes by name). target is a qualified_name (file::ClassName::method) or a bare name for search."
    )]
    async fn query_graph(
        &self,
        params: Parameters<QueryGraphRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::graph::query_graph(self, params.0).await
    }

    #[tool(
        description = "Generate an optimized .voidignore file for the project's semantic vector index. Excludes generated code, build artifacts, test fixtures, and files that don't carry business-logic semantics. Detects project tech stack (Rust, Node, Go, Python, Flutter) for stack-specific patterns. Run before index_project_codebase to improve index quality."
    )]
    async fn generate_voidignore(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::generate_voidignore(self, params.0).await
    }

    #[tool(
        description = "Run Leiden community detection over the semantic index. Non-blocking: returns immediately, work runs in the background. Builds a similarity graph (cosine > 0.72) and groups related chunks into communities. Subsequent semantic_search results include community_id. Requires index to exist (call index_project_codebase first). Poll get_cluster_stats for progress."
    )]
    async fn cluster_project_index(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::cluster_project_index(self, params.0).await
    }

    #[tool(
        description = "Poll the status of the most recent cluster_project_index background job. Returns Idle (nothing run yet), Running (in progress), Completed (with community count), or Failed (with the error message)."
    )]
    async fn get_cluster_stats(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::get_cluster_stats(self, params.0).await
    }

    #[tool(
        description = "Run semantic_search with top_k=10 and group results by Leiden community. Useful for exploring related code clusters. If clustering hasn't run, results appear under 'Unclustered'. Run cluster_project_index first for community grouping."
    )]
    async fn get_communities(
        &self,
        params: Parameters<GetCommunitiesRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::get_communities(self, params.0).await
    }

    #[cfg(all(feature = "vector", feature = "structural"))]
    #[tool(
        description = "GraphRAG: fuse semantic search with the structural call graph. Returns semantic seeds + their callers/callees (BFS up to `depth` hops, max 5 expansions per seed) and a deduplicated, scored 'combined' set. Falls back to silent omission for files not in the semantic index. Requires both build_structural_graph and index_project_codebase to have been run."
    )]
    async fn graph_rag_search(
        &self,
        params: Parameters<GraphRagSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::graph_rag_search(self, params.0).await
    }

    #[cfg(all(feature = "vector", feature = "structural"))]
    #[tool(
        description = "Find dead-code CANDIDATES: structural-graph functions/classes with zero incoming calls that are not entrypoints, tests, trait-impl methods, registered handlers (API contracts), or build scripts. Confidence: high = private + zero callers; medium = exported/pub with no internal callers. Static analysis — reflection/macros/dynamic dispatch are invisible; treat as a review list, not a deletion list. Requires build_structural_graph."
    )]
    async fn find_dead_code(
        &self,
        params: Parameters<DeadCodeRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::review::find_dead_code(self, params.0).await
    }

    #[tool(
        description = "Compact LLM-ready review payload for the current git diff (under ~4k tokens by construction): summary (files/symbols/LOC), audit findings ON CHANGED LINES only (suppression-aware), blast radius (impact BFS depth 2 with hop labels), test coverage for the diff (suggested tests + UNCOVERED symbols), and 1-hop call context for the hottest changed symbols. Call before each commit; address Critical/High findings and decide on the uncovered list. Default base: HEAD; pass git_base (e.g. 'main') for branch reviews. Requires build_structural_graph."
    )]
    async fn review_diff(
        &self,
        params: Parameters<ReviewDiffRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::review::review_diff(self, params.0).await
    }

    #[tool(
        description = "START A SESSION WITH THIS. One call that consolidates the 4-5 usual bootstrap calls: semantic-index stats + structural-graph freshness, docs digest (README/CLAUDE.md first lines), the current git diff with affected symbols, the impact radius of the changed files, and the Doing tasks from BOARD.md. Compact markdown, ~2k tokens max, ready to use as initial context. Sections degrade to 'n/a' hints (e.g. missing index) instead of failing."
    )]
    async fn session_context(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::context::session_context(project).await
    }

    #[tool(
        description = "Show the project's kanban board (BOARD.md at the repo root, versioned in git). Returns the full board as markdown: one section per column (Backlog/Doing/Review/Done), tasks with id, priority, tags, date and linked files/symbols."
    )]
    async fn board_list(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::board::board_list(&project)
    }

    #[tool(
        description = "Add a task to the project's kanban board (BOARD.md, versioned in git). The task goes to Backlog with an auto-assigned short id (VB-n) and today's date. Optional priority (low/medium/high) and tags."
    )]
    async fn board_add_task(
        &self,
        params: Parameters<BoardAddTaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::board::board_add_task(&project, &params.0)
    }

    #[tool(
        description = "Move a kanban task to another column (Backlog, Doing, Review, Done) on the project's BOARD.md. Task ids are case-insensitive (vb-3 == VB-3)."
    )]
    async fn board_move_task(
        &self,
        params: Parameters<BoardMoveTaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tools::board::board_move_task(&project, &params.0)
    }

    #[tool(
        description = "Link files or symbols to a kanban task on BOARD.md. A relative path (src/auth/mod.rs) or symbol (AuthService::login) is linked as-is; a natural-language query is resolved to concrete files through the semantic index. Linked tasks surface automatically in review_diff when a diff touches them."
    )]
    async fn board_link_task(
        &self,
        params: Parameters<BoardLinkTaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config = Self::load_config()?;
        let project = Self::find_project_or_err(&config, &params.0.project)?;
        tokio::task::spawn_blocking(move || tools::board::board_link_task(&project, &params.0))
            .await
            .map_err(|e| McpError::internal_error(format!("board task failed: {}", e), None))?
    }

    #[tool(
        description = "Suggest which tests cover the current git diff, using the structural call graph (reverse coverage map: test -> BFS callees, cached). Returns covering tests ranked by call distance, an explicit UNCOVERED list (changed symbols with zero covering tests), and ready-to-paste runner commands (cargo test -p, go test -run, flutter test, jest). Run these BEFORE the full suite to shorten the loop; run the full suite before the final commit. Requires build_structural_graph. Default diff base: HEAD (working tree + staged); pass git_base for branch diffs."
    )]
    async fn suggest_tests_for_diff(
        &self,
        params: Parameters<SuggestTestsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::review::suggest_tests_for_diff(self, params.0).await
    }

    #[tool(
        description = "List the API contracts a project PRODUCES (proto service/rpc definitions, REST route handlers: Express, FastAPI/Flask, Next.js app router, Go gin/echo/chi/net-http) and CONSUMES (generated gRPC stubs, fetch/axios/Dio HTTP calls). Paths are normalized (/users/:id -> /users/{param}). Useful standalone for architecture review and as the data behind cross-project contract links in graph_rag_search_cross. Cached per file SHA-256 — cheap to call repeatedly."
    )]
    async fn get_api_contracts(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::get_api_contracts(self, params.0).await
    }

    #[tool(
        description = "Cross-project GraphRAG: run graph_rag_search on the primary project, then search the same query in related indexed projects and surface shared symbols as cross-links. Useful for tracing how a feature spans backend/frontend/proxy services (e.g. login implemented in a Go backend, consumed by a Flutter app and a Next.js storefront). Pass related_projects to scope the search (example: {\"project\": \"iunci-flutter\", \"query\": \"google login flow\", \"related_projects\": [\"iunci-backend\", \"iunci.store\"]}). Without it, all indexed projects are searched, matches scoring below 0.65 are dropped, and output is capped to the 5 most relevant projects. Skips related projects without a semantic index — no extra index builds happen here."
    )]
    async fn graph_rag_search_cross(
        &self,
        params: Parameters<GraphRagSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::graph_rag_search_cross(self, params.0).await
    }

    #[tool(
        description = "Generate an interactive `graph.html` (self-contained, no CDN) at {project}/void-stack-out/graph.html. Visualizes module dependencies with layer colors, optional Leiden community ring colors, search/CC filters, layer toggles, click-to-detail panel, and SVG export."
    )]
    async fn generate_graph_html(
        &self,
        params: Parameters<ProjectName>,
    ) -> Result<CallToolResult, McpError> {
        tools::analysis::generate_graph_html(self, params.0).await
    }

    #[tool(
        description = "Run a comprehensive analysis combining security audit, architecture \
                       analysis, and semantic hot-spot detection into a single structured \
                       report. Use this instead of calling audit_project + analyze_project + \
                       semantic_search separately. depth='quick' (~5s), 'standard' (~15s, \
                       default, includes semantic enrichment), 'deep' (~30s, includes \
                       file context). focus=['security','performance','architecture'] to \
                       limit scope. Works with any language supported by void-stack."
    )]
    async fn full_analysis(
        &self,
        params: Parameters<FullAnalysisRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::orchestration::full_analysis(self, params.0).await
    }

    #[tool(
        description = "Manage audit suppressions (.void-audit-ignore) without editing \
                       the file manually. Actions: 'list' shows current rules, 'add' \
                       appends a new rule (idempotent), 'remove' deletes a specific \
                       rule. Changes take effect on the next audit_project run."
    )]
    async fn manage_suppressions(
        &self,
        params: Parameters<ManageSuppressionsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::suppressions::manage_suppressions(self, params.0).await
    }

    #[tool(
        description = "One-shot project setup for new users. Registers the project, \
                       generates .claudeignore and .voidignore, indexes the codebase \
                       for semantic search, and runs a quick analysis. Use this the \
                       FIRST TIME you work with a project — just pass the absolute \
                       path. No CLI required."
    )]
    async fn setup_project(
        &self,
        params: Parameters<SetupProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::setup::setup_project(self, params.0).await
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
