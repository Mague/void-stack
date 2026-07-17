//! MCP tools backed by the structural graph.
//!
//! Each handler takes the full Request struct and resolves the project via
//! `VoidStackMcp::{load_config,find_project_or_err}`, keeping `server.rs`
//! as a one-line router.

use std::time::Duration;

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use super::to_json_pretty;
use crate::server::VoidStackMcp;
use crate::types::{ImpactRadiusRequest, QueryGraphRequest, StructuralBuildRequest};

/// Hard cap on impact-radius response time — beyond this the query has
/// almost certainly hit the IMPORTS_FROM fan-out and will never return.
const IMPACT_TIMEOUT_SECS: u64 = 30;

/// Build (or incrementally update) the structural call graph for a project.
pub async fn build_structural_graph(
    _mcp: &VoidStackMcp,
    req: StructuralBuildRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    let stats = void_stack_core::structural::build_structural_graph(&project, req.force)
        .map_err(|e| McpError::internal_error(e, None))?;
    let json = to_json_pretty(&stats)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Compute the bidirectional blast radius for a set of changed files.
/// Auto-detects via `git diff HEAD~1` when `changed_files` is omitted.
/// `only_calls` defaults to `true` to stay responsive on TS/JS graphs.
pub async fn get_impact_radius(
    _mcp: &VoidStackMcp,
    req: ImpactRadiusRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    let depth = req.max_depth.unwrap_or(2);
    let calls_only = req.only_calls.unwrap_or(true);

    let files = match req.changed_files {
        Some(f) if !f.is_empty() => f,
        _ => {
            let root = std::path::Path::new(&project.path);
            void_stack_core::vector_index::stats::get_git_changed_files(root, "HEAD~1")
        }
    };

    if files.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No changed files to analyze. Pass changed_files or make sure \
             the project is a git repo with commits against HEAD~1."
                .to_string(),
        )]));
    }

    // Run the BFS in a worker thread so a pathological graph can be
    // abandoned after IMPACT_TIMEOUT_SECS instead of blocking the MCP.
    let (tx, rx) = std::sync::mpsc::channel();
    let project_clone = project.clone();
    let files_clone = files.clone();

    std::thread::spawn(move || {
        let result = void_stack_core::structural::open_db(&project_clone).and_then(|conn| {
            void_stack_core::structural::get_impact_radius(
                &conn,
                &files_clone,
                depth,
                500,
                calls_only,
            )
        });
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(IMPACT_TIMEOUT_SECS)) {
        Ok(Ok(impact)) => {
            let json = to_json_pretty(&impact)?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
        Ok(Err(e)) => Err(McpError::internal_error(e, None)),
        Err(_) => Ok(CallToolResult::success(vec![Content::text(format!(
            "Impact radius timed out after {}s. Try only_calls=true (default) \
             to limit traversal to CALLS edges, or lower max_depth to 1.",
            IMPACT_TIMEOUT_SECS
        ))])),
    }
}

/// Query the structural graph: callers, callees, tests, or fuzzy search.
pub async fn query_graph(
    _mcp: &VoidStackMcp,
    req: QueryGraphRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    let conn = void_stack_core::structural::open_db(&project)
        .map_err(|e| McpError::internal_error(e, None))?;

    let nodes = match req.query_type.as_str() {
        "callers" => void_stack_core::structural::get_callers(&conn, &req.target),
        "callees" => void_stack_core::structural::get_callees(&conn, &req.target),
        "tests" => void_stack_core::structural::get_tests_for(&conn, &req.target),
        "search" => void_stack_core::structural::search_nodes(&conn, &req.target, 50),
        other => {
            return Err(McpError::invalid_params(
                format!(
                    "Unknown query_type '{}'. Expected one of: callers, callees, tests, search.",
                    other
                ),
                None,
            ));
        }
    };

    let json = to_json_pretty(&nodes)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use void_stack_core::global_config::{GlobalConfig, save_global_config};
    use void_stack_core::model::Project;

    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    fn project_at(name: &str, path: &str) -> Project {
        Project {
            name: name.to_string(),
            description: String::new(),
            path: path.to_string(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    /// query_graph resolves the project first; an unregistered name is an
    /// invalid-params error before any DB access.
    #[tokio::test]
    async fn test_query_graph_project_not_found() {
        crate::tools::isolate_test_data_dir();
        let _guard = crate::tools::config_test_guard().await;
        save_global_config(&GlobalConfig::default()).unwrap();

        let mcp = VoidStackMcp::new();
        let req = QueryGraphRequest {
            project: "no-such-graph-project".to_string(),
            target: "x".to_string(),
            query_type: "search".to_string(),
        };
        let err = query_graph(&mcp, req).await.unwrap_err();
        assert!(err.message.contains("not found"), "got: {}", err.message);
    }

    /// With no changed files and no git history, get_impact_radius returns the
    /// guidance message instead of touching the graph DB.
    #[tokio::test]
    async fn test_impact_radius_no_changed_files() {
        crate::tools::isolate_test_data_dir();
        let _guard = crate::tools::config_test_guard().await;

        let tmp = tempfile::tempdir().unwrap();
        let name = format!("impact-{}", std::process::id());
        let config = GlobalConfig {
            projects: vec![project_at(&name, &tmp.path().to_string_lossy())],
            ..Default::default()
        };
        save_global_config(&config).unwrap();

        let mcp = VoidStackMcp::new();
        let req = ImpactRadiusRequest {
            project: name.clone(),
            changed_files: Some(vec![]),
            max_depth: None,
            only_calls: None,
        };
        let out = text_of(&get_impact_radius(&mcp, req).await.unwrap());
        assert!(out.contains("No changed files"), "got: {out}");
    }

    /// End-to-end over a minimal structural DB built from one Rust file
    /// (tree-sitter only — no embedding model). Covers build + the query
    /// dispatch arms, including the unknown-query-type guard.
    #[tokio::test]
    async fn test_build_and_query_structural_graph() {
        crate::tools::isolate_test_data_dir();
        let _guard = crate::tools::config_test_guard().await;

        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("main.rs"),
            "fn helper() -> u32 { 42 }\nfn main() { let _ = helper(); }\n",
        )
        .unwrap();

        let name = format!("graph-build-{}", std::process::id());
        let config = GlobalConfig {
            projects: vec![project_at(&name, &tmp.path().to_string_lossy())],
            ..Default::default()
        };
        save_global_config(&config).unwrap();

        let mcp = VoidStackMcp::new();

        // Build the graph.
        let build = build_structural_graph(
            &mcp,
            StructuralBuildRequest {
                project: name.clone(),
                force: true,
            },
        )
        .await
        .unwrap();
        assert!(text_of(&build).contains("files_parsed"));

        // search finds the helper node.
        let search = query_graph(
            &mcp,
            QueryGraphRequest {
                project: name.clone(),
                target: "helper".to_string(),
                query_type: "search".to_string(),
            },
        )
        .await
        .unwrap();
        assert!(
            text_of(&search).contains("helper"),
            "search should find helper"
        );

        // callers/callees/tests dispatch arms all return Ok (possibly empty).
        for qt in ["callers", "callees", "tests"] {
            let res = query_graph(
                &mcp,
                QueryGraphRequest {
                    project: name.clone(),
                    target: "helper".to_string(),
                    query_type: qt.to_string(),
                },
            )
            .await;
            assert!(res.is_ok(), "query_type {qt} should be Ok");
        }

        // Unknown query type is an invalid-params error.
        let err = query_graph(
            &mcp,
            QueryGraphRequest {
                project: name.clone(),
                target: "helper".to_string(),
                query_type: "bogus".to_string(),
            },
        )
        .await
        .unwrap_err();
        assert!(
            err.message.contains("Unknown query_type"),
            "got: {}",
            err.message
        );
    }
}
