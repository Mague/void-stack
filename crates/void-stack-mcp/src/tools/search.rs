//! MCP tools for the vector (semantic) index and its auto-reindex helpers.
//!
//! Same dispatch pattern as `tools/graph.rs`: every public handler takes the
//! full Request struct and resolves the project internally via
//! `VoidStackMcp::{load_config,find_project_or_err}` so `server.rs` stays a
//! one-line router.

use rmcp::ErrorData as McpError;
use rmcp::model::*;

#[cfg(feature = "vector")]
use super::to_json_pretty;
use crate::server::VoidStackMcp;
#[cfg(all(feature = "vector", feature = "structural"))]
use crate::types::GraphRagSearchRequest;
use crate::types::{
    GetCommunitiesRequest, IndexProjectRequest, ProjectName, SemanticSearchRequest,
};

// ── index_project_codebase ──────────────────────────────────

#[cfg(feature = "vector")]
pub async fn index_project_codebase(
    _mcp: &VoidStackMcp,
    req: IndexProjectRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    // Check if already running
    if let Some(void_stack_core::vector_index::IndexJobStatus::Running {
        files_processed,
        files_total,
    }) = void_stack_core::vector_index::get_index_job_status(&project)
    {
        let msg = if files_total > 0 {
            format!(
                "Indexing already in progress: {}/{} files processed ({:.0}%). \
                 Call get_index_stats to check when complete.",
                files_processed,
                files_total,
                files_processed as f64 / files_total as f64 * 100.0
            )
        } else {
            "Indexing already in progress (initializing model...). \
             Call get_index_stats to check when complete."
                .to_string()
        };
        return Ok(CallToolResult::success(vec![Content::text(msg)]));
    }

    let scope_msg = match (req.force, req.git_base.as_deref()) {
        (true, _) => "FORCE mode: re-indexing all files. ".to_string(),
        (false, Some(base)) => format!("Re-indexing only files changed since {}. ", base),
        (false, None) => {
            "Re-indexing only files modified since last index (incremental). ".to_string()
        }
    };

    void_stack_core::vector_index::index_project_background(&project, req.force, req.git_base);

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Indexing started for '{}' (background). {}\
         First run downloads the embedding model (~130MB, one-time). \
         Call get_index_stats in ~30-60 seconds to check progress. \
         Use semantic_search once get_index_stats shows 'created_at'.",
        project.name, scope_msg
    ))]))
}

#[cfg(not(feature = "vector"))]
pub async fn index_project_codebase(
    _mcp: &VoidStackMcp,
    _req: IndexProjectRequest,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── semantic_search ─────────────────────────────────────────

#[cfg(feature = "vector")]
pub async fn semantic_search(
    _mcp: &VoidStackMcp,
    req: SemanticSearchRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;
    let top_k = req.top_k.unwrap_or(5);

    // Validate query has enough content for meaningful embedding
    if req.query.split_whitespace().count() < 2 {
        return Ok(CallToolResult::success(vec![Content::text(
            "Query too short for semantic search. Use at least 2-3 words \
             describing what you're looking for (e.g. 'authentication middleware flow', \
             not just 'auth').",
        )]));
    }

    // CPU-heavy (ONNX embedding + HNSW) behind a sync Mutex — never run it
    // on the async runtime threads.
    let search_project = project.clone();
    let search_query = req.query.clone();
    let results = tokio::task::spawn_blocking(move || {
        void_stack_core::vector_index::semantic_search(&search_project, &search_query, top_k)
    })
    .await
    .map_err(|e| McpError::internal_error(format!("search task failed: {}", e), None))?
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("empty") || msg.contains("0 points") {
            McpError::internal_error(
                format!(
                    "Index appears corrupted or empty. \
                         Run index_project_codebase with force=true to rebuild. \
                         Original error: {}",
                    e
                ),
                None,
            )
        } else {
            McpError::internal_error(format!("Search failed: {}", e), None)
        }
    })?;

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No results found for: \"{}\"",
            req.query
        ))]));
    }

    let mut output = String::new();
    for (i, r) in results.iter().enumerate() {
        output.push_str(&format!(
            "## {}. {} (score: {:.2}, lines {}-{})\n\n```\n{}\n```\n\n",
            i + 1,
            r.file_path,
            r.score,
            r.line_start,
            r.line_end,
            r.chunk
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

#[cfg(not(feature = "vector"))]
pub async fn semantic_search(
    _mcp: &VoidStackMcp,
    _req: SemanticSearchRequest,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── generate_voidignore ─────────────────────────────────────

#[cfg(feature = "vector")]
pub async fn generate_voidignore(
    _mcp: &VoidStackMcp,
    req: ProjectName,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    let project_path = std::path::Path::new(&project.path);
    let result = void_stack_core::vector_index::generate_voidignore(project_path);
    void_stack_core::vector_index::save_voidignore(project_path, &result.content).map_err(|e| {
        McpError::internal_error(format!("Failed to save .voidignore: {}", e), None)
    })?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Generated .voidignore ({} patterns) for project '{}'.\nContent:\n{}",
        result.patterns_count, project.name, result.content
    ))]))
}

#[cfg(not(feature = "vector"))]
pub async fn generate_voidignore(
    _mcp: &VoidStackMcp,
    _req: ProjectName,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── get_index_stats ─────────────────────────────────────────

#[cfg(feature = "vector")]
pub async fn get_index_stats(
    _mcp: &VoidStackMcp,
    req: ProjectName,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    // Check for active/recent job first
    if let Some(status) = void_stack_core::vector_index::get_index_job_status(&project) {
        match status {
            void_stack_core::vector_index::IndexJobStatus::Running {
                files_processed,
                files_total,
            } => {
                // If all files processed, check if index already completed on disk
                // (race condition: index saved to disk but registry not yet updated)
                if files_total > 0 && files_processed >= files_total {
                    if let Ok(Some(stats)) =
                        void_stack_core::vector_index::get_index_stats(&project)
                    {
                        let json = to_json_pretty(&stats)?;
                        return Ok(CallToolResult::success(vec![Content::text(json)]));
                    }
                    // Still generating embeddings/HNSW
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Status: INDEXING IN PROGRESS\n\
                         Files read: {}/{} (100%)\n\
                         Generating embeddings and building HNSW index...\n\
                         Call get_index_stats again in 30-60 seconds.",
                        files_processed, files_total
                    ))]));
                }

                let msg = if files_total > 0 {
                    format!(
                        "Status: INDEXING IN PROGRESS\nFiles: {}/{} ({:.0}%)\n\
                         The index will be ready when this completes.",
                        files_processed,
                        files_total,
                        files_processed as f64 / files_total as f64 * 100.0
                    )
                } else {
                    "Status: INDEXING IN PROGRESS\nInitializing embedding model...".to_string()
                };
                return Ok(CallToolResult::success(vec![Content::text(msg)]));
            }
            void_stack_core::vector_index::IndexJobStatus::Failed { error } => {
                return Err(McpError::internal_error(
                    format!("Last indexing failed: {}", error),
                    None,
                ));
            }
            void_stack_core::vector_index::IndexJobStatus::Completed { .. } => {
                // Fall through to read stats from disk
            }
        }
    }

    // Normal stats read from disk
    match void_stack_core::vector_index::get_index_stats(&project) {
        Ok(Some(stats)) => {
            let json = to_json_pretty(&stats)?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
        Ok(None) => Ok(CallToolResult::success(vec![Content::text(format!(
            "No index found for '{}'. Run index_project_codebase first.",
            project.name
        ))])),
        Err(e) => Err(McpError::internal_error(
            format!("Failed to load index stats: {}", e),
            None,
        )),
    }
}

#[cfg(not(feature = "vector"))]
pub async fn get_index_stats(
    _mcp: &VoidStackMcp,
    _req: ProjectName,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── watch / unwatch ─────────────────────────────────────────

#[cfg(feature = "vector")]
pub async fn watch_project(
    _mcp: &VoidStackMcp,
    req: ProjectName,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    if void_stack_core::vector_index::is_watching(&project) {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "Already watching '{}'. File changes trigger automatic re-indexing.",
            project.name
        ))]));
    }

    void_stack_core::vector_index::watch_project(&project)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Watch started for '{}'. The semantic index will update automatically \
         within ~500ms of any file change. Call unwatch_project to stop.",
        project.name
    ))]))
}

#[cfg(not(feature = "vector"))]
pub async fn watch_project(
    _mcp: &VoidStackMcp,
    _req: ProjectName,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

#[cfg(feature = "vector")]
pub async fn unwatch_project(
    _mcp: &VoidStackMcp,
    req: ProjectName,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    void_stack_core::vector_index::unwatch_project(&project);
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Watch stopped for '{}'.",
        project.name
    ))]))
}

#[cfg(not(feature = "vector"))]
pub async fn unwatch_project(
    _mcp: &VoidStackMcp,
    _req: ProjectName,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── install_index_hook ──────────────────────────────────────

#[cfg(feature = "vector")]
pub async fn install_index_hook(
    _mcp: &VoidStackMcp,
    req: ProjectName,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    void_stack_core::vector_index::install_git_hook(&project)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Post-commit hook installed for '{}'. Each `git commit` now triggers an \
         incremental re-index of files changed since HEAD.",
        project.name
    ))]))
}

#[cfg(not(feature = "vector"))]
pub async fn install_index_hook(
    _mcp: &VoidStackMcp,
    _req: ProjectName,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── cluster_project_index ───────────────────────────────────

#[cfg(feature = "vector")]
pub async fn cluster_project_index(
    _mcp: &VoidStackMcp,
    req: ProjectName,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    // Synchronous clustering used to exceed the MCP tool timeout on
    // mid-size projects (>4 min). The job now runs in a background thread
    // and progress is polled via get_cluster_stats.
    void_stack_core::vector_index::cluster_project_background(&project)
        .map_err(|e| McpError::internal_error(format!("Clustering failed: {}", e), None))?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Clustering started for '{}' in background. \
         Call get_cluster_stats to check progress.",
        project.name
    ))]))
}

#[cfg(not(feature = "vector"))]
pub async fn cluster_project_index(
    _mcp: &VoidStackMcp,
    _req: ProjectName,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── get_cluster_stats ───────────────────────────────────────

#[cfg(feature = "vector")]
pub async fn get_cluster_stats(
    _mcp: &VoidStackMcp,
    _req: ProjectName,
) -> Result<CallToolResult, McpError> {
    use void_stack_core::vector_index::ClusterJobState;

    let msg = match void_stack_core::vector_index::get_cluster_job_state() {
        ClusterJobState::Idle => {
            "No clustering job has run yet. Call cluster_project_index first.".to_string()
        }
        ClusterJobState::Running => "Clustering in progress...".to_string(),
        ClusterJobState::Completed { communities } => {
            format!("Completed — {communities} communities detected.")
        }
        ClusterJobState::Failed(e) => format!("Clustering failed: {e}"),
    };
    Ok(CallToolResult::success(vec![Content::text(msg)]))
}

#[cfg(not(feature = "vector"))]
pub async fn get_cluster_stats(
    _mcp: &VoidStackMcp,
    _req: ProjectName,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── get_communities ─────────────────────────────────────────

#[cfg(feature = "vector")]
pub async fn get_communities(
    _mcp: &VoidStackMcp,
    req: GetCommunitiesRequest,
) -> Result<CallToolResult, McpError> {
    use std::collections::BTreeMap;

    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;

    if req.query.split_whitespace().count() < 2 {
        return Ok(CallToolResult::success(vec![Content::text(
            "Query too short. Use at least 2-3 words describing what you're looking for.",
        )]));
    }

    let search_project = project.clone();
    let search_query = req.query.clone();
    let results = tokio::task::spawn_blocking(move || {
        void_stack_core::vector_index::semantic_search(&search_project, &search_query, 10)
    })
    .await
    .map_err(|e| McpError::internal_error(format!("search task failed: {}", e), None))?
    .map_err(|e| McpError::internal_error(format!("Search failed: {}", e), None))?;

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No results found for: \"{}\"",
            req.query
        ))]));
    }

    // BTreeMap so output is sorted by community_id ascending; None goes last.
    let mut by_community: BTreeMap<Option<usize>, Vec<&_>> = BTreeMap::new();
    for r in &results {
        by_community.entry(r.community_id).or_default().push(r);
    }

    let mut output = String::new();
    for (community_id, members) in &by_community {
        let header = match community_id {
            Some(id) => format!("## Community {} ({} members)", id, members.len()),
            None => format!("## Unclustered ({} chunks)", members.len()),
        };
        output.push_str(&header);
        output.push('\n');
        for r in members {
            output.push_str(&format!(
                "- `{}` lines {}-{} (score {:.2})\n",
                r.file_path, r.line_start, r.line_end, r.score
            ));
        }
        output.push_str("\n```\n");
        for r in members {
            output.push_str(&r.chunk);
            output.push_str("\n---\n");
        }
        output.push_str("```\n\n");
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

#[cfg(not(feature = "vector"))]
pub async fn get_communities(
    _mcp: &VoidStackMcp,
    _req: GetCommunitiesRequest,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "Vector search not available. Rebuild with --features vector".to_string(),
        None,
    ))
}

// ── graph_rag_search ────────────────────────────────────────

/// Notices that explain *why* structural context may be missing or
/// incomplete, instead of silently showing "Structural Context (0)".
#[cfg(all(feature = "vector", feature = "structural"))]
fn graph_rag_notices(has_structural_index: bool, files_skipped_not_indexed: usize) -> String {
    let mut out = String::new();
    if !has_structural_index {
        out.push_str(
            "\n⚠️ Structural graph not built for this project — run `build_structural_graph` \
             to enable caller/callee expansion.\n",
        );
    }
    if files_skipped_not_indexed > 0 {
        out.push_str(&format!(
            "\n*{} file(s) skipped (not in semantic index) — re-run `index_project` to include them.*\n",
            files_skipped_not_indexed
        ));
    }
    out
}

#[cfg(all(feature = "vector", feature = "structural"))]
pub async fn graph_rag_search(
    _mcp: &VoidStackMcp,
    req: GraphRagSearchRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;
    let top_k = req.top_k.unwrap_or(5);
    let depth = req.depth.unwrap_or(2);

    if req.query.split_whitespace().count() < 2 {
        return Ok(CallToolResult::success(vec![Content::text(
            "Query too short. Use at least 2-3 words describing what you're looking for.",
        )]));
    }

    let search_project = project.clone();
    let search_query = req.query.clone();
    let result = tokio::task::spawn_blocking(move || {
        void_stack_core::vector_index::graph_rag_search(
            &search_project,
            &search_query,
            top_k,
            depth,
        )
    })
    .await
    .map_err(|e| McpError::internal_error(format!("search task failed: {}", e), None))?
    .map_err(|e| McpError::internal_error(format!("graph-rag failed: {}", e), None))?;

    let mut output = String::new();

    output.push_str(&format!(
        "## Semantic Seeds ({})\n",
        result.semantic_seeds.len()
    ));
    for (i, r) in result.semantic_seeds.iter().enumerate() {
        let community = match r.community_id {
            Some(c) => format!(" [community {}]", c),
            None => String::new(),
        };
        output.push_str(&format!(
            "{}. `{}` lines {}-{} (score {:.2}){}\n\n```\n{}\n```\n\n",
            i + 1,
            r.file_path,
            r.line_start,
            r.line_end,
            r.score,
            community,
            r.chunk
        ));
    }

    output.push_str(&format!(
        "## Structural Context ({})\n",
        result.structural_context.len()
    ));
    output.push_str(&graph_rag_notices(
        result.has_structural_index,
        result.files_skipped_not_indexed,
    ));
    for (i, c) in result.structural_context.iter().enumerate() {
        let src = match c.source {
            void_stack_core::vector_index::ContextSource::Caller => "caller",
            void_stack_core::vector_index::ContextSource::Callee => "callee",
            void_stack_core::vector_index::ContextSource::TestFor => "test",
        };
        output.push_str(&format!(
            "{}. [{}/hop {}] `{}` lines {}-{}\n\n```\n{}\n```\n\n",
            i + 1,
            src,
            c.hops,
            c.file_path,
            c.line_start,
            c.line_end,
            c.chunk
        ));
    }

    output.push_str(&format!(
        "\n~{} tokens | {} semantic + {} structural chunks\n",
        result.token_estimate,
        result.semantic_seeds.len(),
        result.structural_context.len()
    ));

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

#[cfg(not(all(feature = "vector", feature = "structural")))]
pub async fn graph_rag_search(
    _mcp: &VoidStackMcp,
    _req: crate::types::GetCommunitiesRequest,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "graph_rag_search requires both `vector` and `structural` features".to_string(),
        None,
    ))
}

// ── graph_rag_search_cross ──────────────────────────────────

#[cfg(all(feature = "vector", feature = "structural"))]
pub async fn graph_rag_search_cross(
    _mcp: &VoidStackMcp,
    req: GraphRagSearchRequest,
) -> Result<CallToolResult, McpError> {
    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;
    let top_k = req.top_k.unwrap_or(5);
    let depth = req.depth.unwrap_or(2);

    if req.query.split_whitespace().count() < 2 {
        return Ok(CallToolResult::success(vec![Content::text(
            "Query too short. Use at least 2-3 words describing what you're looking for.",
        )]));
    }

    let search_config = config.clone();
    let search_project = project.clone();
    let search_query = req.query.clone();
    let result = tokio::task::spawn_blocking(move || {
        void_stack_core::vector_index::graph_rag_search_cross(
            &search_config,
            &search_project,
            &search_query,
            top_k,
            depth,
        )
    })
    .await
    .map_err(|e| McpError::internal_error(format!("search task failed: {}", e), None))?
    .map_err(|e| McpError::internal_error(format!("cross-project graphrag failed: {}", e), None))?;

    let mut output = String::new();

    // Primary section reuses the same markdown shape as graph_rag_search.
    let primary = &result.primary;
    output.push_str(&format!(
        "## Primary: {} — Semantic Seeds ({})\n",
        project.name,
        primary.semantic_seeds.len()
    ));
    for (i, r) in primary.semantic_seeds.iter().enumerate() {
        output.push_str(&format!(
            "{}. `{}` lines {}-{} (score {:.2})\n\n```\n{}\n```\n\n",
            i + 1,
            r.file_path,
            r.line_start,
            r.line_end,
            r.score,
            r.chunk
        ));
    }

    if !result.related.is_empty() {
        output.push_str("\n## Related Projects\n");
        for (proj_name, hits) in &result.related {
            // Look up the `via` for this project, if any.
            let via = result
                .cross_links
                .iter()
                .find(|l| l.to_project.eq_ignore_ascii_case(proj_name))
                .map(|l| l.via.as_str())
                .unwrap_or("no shared symbols");
            output.push_str(&format!("\n### {} (via: {})\n", proj_name, via));
            for (i, r) in hits.iter().enumerate() {
                output.push_str(&format!(
                    "{}. `{}` lines {}-{} (score {:.2})\n\n```\n{}\n```\n\n",
                    i + 1,
                    r.file_path,
                    r.line_start,
                    r.line_end,
                    r.score,
                    r.chunk
                ));
            }
        }
    }

    if !result.cross_links.is_empty() {
        output.push_str("\n## Shared Symbols\n");
        for link in &result.cross_links {
            output.push_str(&format!(
                "- {} → {} (via {}): {}\n",
                link.from_project,
                link.to_project,
                link.via,
                link.shared_symbols.join(", ")
            ));
        }
    }

    output.push_str(&graph_rag_notices(
        primary.has_structural_index,
        primary.files_skipped_not_indexed,
    ));

    output.push_str(&format!(
        "\n~{} tokens | primary: {} seeds + {} structural | related: {} projects\n",
        primary.token_estimate,
        primary.semantic_seeds.len(),
        primary.structural_context.len(),
        result.related.len(),
    ));

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

#[cfg(not(all(feature = "vector", feature = "structural")))]
pub async fn graph_rag_search_cross(
    _mcp: &VoidStackMcp,
    _req: crate::types::GetCommunitiesRequest,
) -> Result<CallToolResult, McpError> {
    Err(McpError::invalid_params(
        "graph_rag_search_cross requires both `vector` and `structural` features".to_string(),
        None,
    ))
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(all(test, feature = "vector"))]
mod tests {
    use super::*;
    use crate::types::{IndexProjectRequest, ProjectName, SemanticSearchRequest};

    fn mcp() -> VoidStackMcp {
        VoidStackMcp::new()
    }

    // Note: these tests exercise the tool layer against the user's real
    // GlobalConfig — if `test-project` doesn't exist, find_project_or_err
    // returns invalid_params, which is still Ok-as-Err from our side (the
    // handler doesn't panic). That is what we're checking: the dispatch
    // doesn't blow up.

    #[cfg(feature = "structural")]
    #[test]
    fn test_graph_rag_notices_missing_graph() {
        let notice = graph_rag_notices(false, 0);
        assert!(notice.contains("Structural graph not built"));
        assert!(notice.contains("build_structural_graph"));
    }

    #[cfg(feature = "structural")]
    #[test]
    fn test_graph_rag_notices_skipped_files() {
        let notice = graph_rag_notices(true, 7);
        assert!(notice.contains("7 file(s) skipped (not in semantic index)"));
    }

    #[cfg(feature = "structural")]
    #[test]
    fn test_graph_rag_notices_clean_when_all_present() {
        assert!(graph_rag_notices(true, 0).is_empty());
    }

    #[tokio::test]
    async fn test_index_project_codebase_dispatch() {
        let req = IndexProjectRequest {
            project: "nonexistent-xyz-project".to_string(),
            force: false,
            git_base: None,
        };
        // Either Ok (project found, background job kicked off) or Err with
        // "Project 'nonexistent-xyz-project' not found". Neither panics.
        let _ = index_project_codebase(&mcp(), req).await;
    }

    #[tokio::test]
    async fn test_semantic_search_short_query_returns_hint() {
        let req = SemanticSearchRequest {
            project: "nonexistent-xyz-project".to_string(),
            query: "x".to_string(),
            top_k: Some(5),
        };
        // Project lookup may fail before the query-length check, but either
        // way we should not panic.
        let _ = semantic_search(&mcp(), req).await;
    }

    #[tokio::test]
    async fn test_get_index_stats_dispatch() {
        let req = ProjectName {
            project: "nonexistent-xyz-project".to_string(),
        };
        let _ = get_index_stats(&mcp(), req).await;
    }
}
