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
pub(crate) struct CommitDetailRequest {
    /// Name of the project (case-insensitive)
    pub project: String,
    /// Commit hash (short or full, hex only)
    pub hash: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── request deserialization: defaults fill in ───────────

    #[test]
    fn test_logs_request_defaults_and_overrides() {
        let req: LogsRequest =
            serde_json::from_value(json!({"project": "demo", "service": "api"})).unwrap();
        assert_eq!(req.lines, 50, "lines defaults to 50");
        assert!(!req.raw, "raw defaults to false");

        let req: LogsRequest = serde_json::from_value(
            json!({"project": "demo", "service": "api", "lines": 200, "raw": true}),
        )
        .unwrap();
        assert_eq!(req.lines, 200);
        assert!(req.raw);
    }

    #[test]
    fn test_logs_request_missing_required_field_fails() {
        assert!(serde_json::from_value::<LogsRequest>(json!({"project": "demo"})).is_err());
        assert!(serde_json::from_value::<LogsRequest>(json!({})).is_err());
    }

    #[test]
    fn test_read_docs_request_default_filename() {
        let req: ReadDocsRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert_eq!(req.filename, "README.md");

        let req: ReadDocsRequest =
            serde_json::from_value(json!({"project": "demo", "filename": "CHANGELOG.md"})).unwrap();
        assert_eq!(req.filename, "CHANGELOG.md");
    }

    #[test]
    fn test_add_project_request_wsl_defaults() {
        let req: AddProjectRequest =
            serde_json::from_value(json!({"name": "demo", "path": "C:\\p"})).unwrap();
        assert!(!req.wsl, "wsl defaults to false");
        assert!(req.distro.is_none());

        let req: AddProjectRequest = serde_json::from_value(
            json!({"name": "demo", "path": "/home/u/p", "wsl": true, "distro": "Ubuntu"}),
        )
        .unwrap();
        assert!(req.wsl);
        assert_eq!(req.distro.as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn test_index_project_request_defaults() {
        let req: IndexProjectRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(!req.force);
        assert!(req.git_base.is_none());

        let req: IndexProjectRequest =
            serde_json::from_value(json!({"project": "demo", "force": true, "git_base": "HEAD~1"}))
                .unwrap();
        assert!(req.force);
        assert_eq!(req.git_base.as_deref(), Some("HEAD~1"));
    }

    #[test]
    fn test_full_analysis_request_optionals_default_to_none() {
        let req: FullAnalysisRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.depth.is_none());
        assert!(req.focus.is_none());

        let req: FullAnalysisRequest = serde_json::from_value(
            json!({"project": "demo", "depth": "deep", "focus": ["security"]}),
        )
        .unwrap();
        assert_eq!(req.depth.as_deref(), Some("deep"));
        assert_eq!(req.focus.as_deref(), Some(&["security".to_string()][..]));
    }

    #[test]
    fn test_board_requests_optional_fields() {
        let req: BoardAddTaskRequest =
            serde_json::from_value(json!({"project": "demo", "title": "Fix"})).unwrap();
        assert!(req.priority.is_none());
        assert!(req.tags.is_none());

        let req: BoardHistoryRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.id.is_none());

        let req: BoardTimelineRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.by.is_none());
        assert!(req.since.is_none());

        let req: BoardTimelineRequest = serde_json::from_value(
            json!({"project": "demo", "by": "week", "since": "3 months ago"}),
        )
        .unwrap();
        assert_eq!(req.by.as_deref(), Some("week"));
        assert_eq!(req.since.as_deref(), Some("3 months ago"));
    }

    #[test]
    fn test_setup_project_request_defaults() {
        let req: SetupProjectRequest = serde_json::from_value(json!({"path": "C:\\p"})).unwrap();
        assert!(req.name.is_none());
        assert!(req.wsl.is_none());
        assert!(req.distro.is_none());
        assert!(req.include_diagrams.is_none());
    }

    #[test]
    fn test_manage_suppressions_request_defaults() {
        let req: ManageSuppressionsRequest =
            serde_json::from_value(json!({"project": "demo", "action": "list"})).unwrap();
        assert_eq!(req.action, "list");
        assert!(req.rule.is_none());
        assert!(req.path.is_none());
    }

    #[test]
    fn test_token_stats_request_all_optional_except_none() {
        // Both fields are optional — an empty object is valid.
        let req: TokenStatsRequest = serde_json::from_value(json!({})).unwrap();
        assert!(req.project.is_none());
        assert!(req.days.is_none());
    }

    // ── response serialization ──────────────────────────────

    #[test]
    fn test_service_info_omits_absent_docker_fields() {
        let info = ServiceInfo {
            name: "api".into(),
            command: "npm run dev".into(),
            target: "windows".into(),
            working_dir: None,
            enabled: true,
            docker_ports: None,
            docker_volumes: None,
        };
        let v = serde_json::to_value(&info).unwrap();
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("docker_ports"));
        assert!(!obj.contains_key("docker_volumes"));
        // Non-optional fields serialize normally.
        assert_eq!(v["name"], "api");
        assert_eq!(v["enabled"], true);
        assert_eq!(v["working_dir"], serde_json::Value::Null);
    }

    #[test]
    fn test_start_stop_result_shape() {
        let result = StartStopResult {
            project: "demo".into(),
            results: vec![ServiceStateInfo {
                name: "api".into(),
                status: "running".into(),
                pid: Some(1234),
                url: Some("http://localhost:3000".into()),
                last_log: None,
            }],
        };
        let v = serde_json::to_value(&result).unwrap();
        assert_eq!(v["project"], "demo");
        assert_eq!(v["results"][0]["pid"], 1234);
        assert_eq!(v["results"][0]["status"], "running");
    }

    // ── more request deserialization ────────────────────────

    #[test]
    fn test_semantic_search_request_defaults() {
        let req: SemanticSearchRequest =
            serde_json::from_value(json!({"project": "demo", "query": "auth flow"})).unwrap();
        assert_eq!(req.query, "auth flow");
        assert!(req.top_k.is_none());
        assert!(req.mode.is_none(), "mode defaults to None");

        let req: SemanticSearchRequest = serde_json::from_value(
            json!({"project": "demo", "query": "x", "top_k": 3, "mode": "lexical"}),
        )
        .unwrap();
        assert_eq!(req.top_k, Some(3));
        assert_eq!(req.mode.as_deref(), Some("lexical"));
    }

    #[test]
    fn test_audit_and_analyze_request_optionals() {
        let req: AuditRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.verbose.is_none());
        let req: AuditRequest =
            serde_json::from_value(json!({"project": "demo", "verbose": true})).unwrap();
        assert_eq!(req.verbose, Some(true));

        let req: AnalyzeRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.service.is_none());
        assert!(req.best_practices.is_none());
        assert!(req.compact.is_none());
        let req: AnalyzeRequest = serde_json::from_value(
            json!({"project": "demo", "service": "api", "best_practices": true, "compact": true}),
        )
        .unwrap();
        assert_eq!(req.service.as_deref(), Some("api"));
        assert_eq!(req.best_practices, Some(true));
        assert_eq!(req.compact, Some(true));
    }

    #[test]
    fn test_diagram_request_optional_format() {
        let req: DiagramRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.format.is_none());
        let req: DiagramRequest =
            serde_json::from_value(json!({"project": "demo", "format": "mermaid"})).unwrap();
        assert_eq!(req.format.as_deref(), Some("mermaid"));
    }

    #[test]
    fn test_suggest_request_optionals() {
        let req: SuggestRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.model.is_none());
        assert!(req.service.is_none());
        let req: SuggestRequest = serde_json::from_value(
            json!({"project": "demo", "model": "llama3.2", "service": "web"}),
        )
        .unwrap();
        assert_eq!(req.model.as_deref(), Some("llama3.2"));
        assert_eq!(req.service.as_deref(), Some("web"));
    }

    #[test]
    fn test_debt_requests_optionals() {
        let req: SaveDebtRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.label.is_none());

        let req: CompareDebtRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.index_a.is_none());
        assert!(req.index_b.is_none());
        let req: CompareDebtRequest =
            serde_json::from_value(json!({"project": "demo", "index_a": 0, "index_b": 2})).unwrap();
        assert_eq!(req.index_a, Some(0));
        assert_eq!(req.index_b, Some(2));
    }

    #[test]
    fn test_docker_and_claudeignore_request_optionals() {
        let req: DockerGenerateRequest =
            serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.generate_dockerfile.is_none());
        assert!(req.generate_compose.is_none());
        assert!(req.save.is_none());

        let req: GenerateClaudeIgnoreRequest =
            serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.dry_run.is_none());
        let req: GenerateClaudeIgnoreRequest =
            serde_json::from_value(json!({"project": "demo", "dry_run": true})).unwrap();
        assert_eq!(req.dry_run, Some(true));
    }

    #[test]
    fn test_add_service_request_required_and_optional() {
        let req: AddServiceRequest = serde_json::from_value(json!({
            "project": "demo",
            "name": "db",
            "command": "postgres:16",
            "working_dir": "C:/ws/demo"
        }))
        .unwrap();
        assert_eq!(req.name, "db");
        assert!(req.target.is_none());
        assert!(req.docker_ports.is_none());

        // Missing a required field fails.
        assert!(
            serde_json::from_value::<AddServiceRequest>(
                json!({"project": "demo", "name": "db", "command": "x"})
            )
            .is_err()
        );
    }

    #[test]
    fn test_diff_family_requests_defaults() {
        let req: SuggestTestsRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.git_base.is_none());
        assert!(req.max_results.is_none());

        let req: ReviewDiffRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.git_base.is_none());

        let req: DeadCodeRequest =
            serde_json::from_value(json!({"project": "demo", "max_results": 10})).unwrap();
        assert_eq!(req.max_results, Some(10));
    }

    #[test]
    fn test_service_ref_and_scan_and_commit_requests() {
        let req: ServiceRef =
            serde_json::from_value(json!({"project": "demo", "service": "api"})).unwrap();
        assert_eq!(req.service, "api");

        let req: ScanDirectoryRequest = serde_json::from_value(json!({"path": "C:/ws"})).unwrap();
        assert_eq!(req.path, "C:/ws");

        let req: CommitDetailRequest =
            serde_json::from_value(json!({"project": "demo", "hash": "abc123"})).unwrap();
        assert_eq!(req.hash, "abc123");
    }

    #[test]
    fn test_board_move_archive_and_update_requests() {
        let req: BoardMoveTaskRequest =
            serde_json::from_value(json!({"project": "demo", "id": "VB-3", "column": "Doing"}))
                .unwrap();
        assert_eq!(req.id, "VB-3");
        assert_eq!(req.column, "Doing");

        let req: BoardArchiveRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.days.is_none());
        let req: BoardArchiveRequest =
            serde_json::from_value(json!({"project": "demo", "days": 30})).unwrap();
        assert_eq!(req.days, Some(30));

        let req: UpdateProjectRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.new_name.is_none());
        assert!(req.new_path.is_none());
    }

    #[test]
    fn test_sync_todos_and_handoff_and_link_requests() {
        let req: SyncTodosRequest = serde_json::from_value(json!({"project": "demo"})).unwrap();
        assert!(req.clean.is_none());

        let req: SessionHandoffRequest =
            serde_json::from_value(json!({"project": "demo", "note": "wip"})).unwrap();
        assert_eq!(req.note.as_deref(), Some("wip"));

        let req: BoardLinkTaskRequest = serde_json::from_value(
            json!({"project": "demo", "id": "VB-1", "query": "src/main.rs"}),
        )
        .unwrap();
        assert_eq!(req.query, "src/main.rs");
    }

    #[test]
    fn test_daily_briefing_request_defaults() {
        let req: DailyBriefingRequest = serde_json::from_value(json!({})).unwrap();
        assert!(req.projects.is_none());
        assert!(req.save.is_none());
    }

    // ── more response serialization ─────────────────────────

    #[test]
    fn test_project_info_serializes_nested_services() {
        let info = ProjectInfo {
            name: "demo".into(),
            path: "C:/ws/demo".into(),
            project_type: "Rust".into(),
            services: vec![ServiceInfo {
                name: "api".into(),
                command: "cargo run".into(),
                target: "windows".into(),
                working_dir: Some("C:/ws/demo/api".into()),
                enabled: true,
                docker_ports: Some(vec!["5432:5432".into()]),
                docker_volumes: None,
            }],
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["project_type"], "Rust");
        assert_eq!(v["services"][0]["docker_ports"][0], "5432:5432");
        // docker_volumes was None → skipped.
        assert!(
            !v["services"][0]
                .as_object()
                .unwrap()
                .contains_key("docker_volumes")
        );
    }

    #[test]
    fn test_service_state_info_null_optionals() {
        let info = ServiceStateInfo {
            name: "api".into(),
            status: "stopped".into(),
            pid: None,
            url: None,
            last_log: None,
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["status"], "stopped");
        assert_eq!(v["pid"], serde_json::Value::Null);
        assert_eq!(v["url"], serde_json::Value::Null);
    }
}
