//! Request and response types for all MCP tools.
//!
//! Extracted from `server.rs` so the router only carries dispatch logic.
//! `*Request` structs deserialize tool parameters (with `JsonSchema` for
//! auto-discovery); response structs (`*Info`, `StartStopResult`) are the
//! JSON payloads the `tools/` handlers serialize back to callers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── serde default helpers ──────────────────────────────────

fn default_log_lines() -> usize {
    50
}

fn default_doc_file() -> String {
    "README.md".to_string()
}

// ── Tool parameter types ────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ProjectName {
    /// Name of the project (case-insensitive)
    pub project: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SyncTodosRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Purge synced tasks whose marker no longer passes the comment-only
    /// filter (default: false — stale tasks resolve to Done instead)
    pub clean: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SessionHandoffRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Free-form note to open the handoff with (e.g. "stopping mid-refactor")
    pub note: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct DailyBriefingRequest {
    /// Projects to cover; omit to use the configured active list
    pub projects: Option<Vec<String>>,
    /// Also save to <data dir>/void-stack/briefings/YYYY-MM-DD.md (default: false)
    pub save: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct BoardArchiveRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Archive Done tasks older than this many days (default: 14)
    pub days: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct BoardAddTaskRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Task title
    pub title: String,
    /// Priority: low, medium or high
    pub priority: Option<String>,
    /// Tags without the leading '#'
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct BoardMoveTaskRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Task id (e.g. VB-3)
    pub id: String,
    /// Target column: Backlog, Doing, Review or Done
    pub column: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct BoardHistoryRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Task id (e.g. VB-3) for one task's full detail; omit for the
    /// whole-board history
    pub id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct BoardTimelineRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Grouping: day, week (alias: sprint), month, year, type or
    /// scope (alias: area). Default: month
    pub by: Option<String>,
    /// Only include work after this point ("2026-01-01", "3 months ago")
    pub since: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct BoardLinkTaskRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Task id (e.g. VB-3)
    pub id: String,
    /// What to link: a relative file path or symbol name is linked as-is;
    /// anything else is resolved to files through the semantic index
    pub query: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct UpdateProjectRequest {
    /// Current name of the project (case-insensitive)
    pub project: String,
    /// New name for the project (omit to keep the current name)
    pub new_name: Option<String>,
    /// New path for the project — move the directory first, then call this
    /// (omit to keep the current path)
    pub new_path: Option<String>,
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
    /// Search mode: "hybrid" (BM25 + vector, default), "vector", "lexical"
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct GetCommunitiesRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Natural language query — results are grouped by Leiden community
    pub query: String,
}

#[cfg(all(feature = "vector", feature = "structural"))]
#[derive(Deserialize, JsonSchema)]
pub(crate) struct GraphRagSearchRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Natural language query
    pub query: String,
    /// Number of semantic seeds to retrieve (default: 5)
    #[serde(default)]
    pub top_k: Option<usize>,
    /// BFS depth across the call graph (default: 2, max 3)
    #[serde(default)]
    pub depth: Option<u8>,
    /// Only search these related projects (graph_rag_search_cross only).
    /// Example: ["iunci-backend", "iunci.store"]. When omitted, all indexed
    /// projects are searched but weak matches are floored and the output is
    /// capped to the 5 most relevant projects.
    #[serde(default)]
    pub related_projects: Option<Vec<String>>,
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

#[derive(Deserialize, JsonSchema)]
pub(crate) struct FullAnalysisRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Analysis depth: "quick" (audit + analyze, ~5s),
    /// "standard" (+ hot-spot enrichment via semantic search, ~15s),
    /// "deep" (+ full file reads for top hot spots, ~30s).
    #[serde(default)]
    pub depth: Option<String>,
    /// Focus areas. Default: all three.
    /// Options: "security", "performance", "architecture".
    #[serde(default)]
    pub focus: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ManageSuppressionsRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Action: "list", "add", or "remove"
    pub action: String,
    /// Rule pattern (required for add/remove). Supports wildcards: "unwrap-*", "CC-*", "*".
    #[serde(default)]
    pub rule: Option<String>,
    /// File path glob (required for add/remove). Example: "crates/**/vuln_patterns/**"
    #[serde(default)]
    pub path: Option<String>,
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

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetupProjectRequest {
    /// Absolute path to the project directory
    pub path: String,
    /// Project name (optional — defaults to folder name)
    #[serde(default)]
    pub name: Option<String>,
    /// True if the project is inside WSL
    #[serde(default)]
    pub wsl: Option<bool>,
    /// WSL distro name
    #[serde(default)]
    pub distro: Option<String>,
    /// Generate architecture diagrams (default: false)
    #[serde(default)]
    pub include_diagrams: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct SuggestTestsRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Git ref to diff against (default: HEAD = working tree + staged)
    #[serde(default)]
    pub git_base: Option<String>,
    /// Max suggested tests (default: 20)
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct ReviewDiffRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Git ref to diff against (default: HEAD = working tree + staged)
    #[serde(default)]
    pub git_base: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct DeadCodeRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Max candidates returned (default: 50)
    #[serde(default)]
    pub max_results: Option<usize>,
}
