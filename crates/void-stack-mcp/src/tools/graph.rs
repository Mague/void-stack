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
