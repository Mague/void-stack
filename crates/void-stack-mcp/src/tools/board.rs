//! Board tools: kanban board stored as BOARD.md in the managed repo.

use std::path::PathBuf;

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::board;
use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

use crate::types::{
    BoardAddTaskRequest, BoardHistoryRequest, BoardLinkTaskRequest, BoardMoveTaskRequest,
};

fn root_of(project: &Project) -> PathBuf {
    PathBuf::from(strip_win_prefix(&project.path))
}

fn load(project: &Project) -> Result<(board::Board, PathBuf), McpError> {
    let root = root_of(project);
    let b =
        board::load_board(&root, &project.name).map_err(|e| McpError::internal_error(e, None))?;
    Ok((b, root))
}

fn save(root: &std::path::Path, b: &board::Board) -> Result<(), McpError> {
    board::save_board(root, b).map_err(|e| McpError::internal_error(e, None))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Logic for board_list tool — returns the canonical markdown, which is
/// both the storage format and a fine LLM payload.
pub fn board_list(project: &Project) -> Result<CallToolResult, McpError> {
    let (b, root) = load(project)?;
    let mut out = board::board_to_markdown(&b);
    out.push_str(&format!(
        "\n_(file: {})_\n",
        board::board_path(&root).display()
    ));
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Logic for board_add_task tool.
pub fn board_add_task(
    project: &Project,
    req: &BoardAddTaskRequest,
) -> Result<CallToolResult, McpError> {
    let (mut b, root) = load(project)?;
    let tags = req.tags.clone().unwrap_or_default();
    let id = board::add_task(&mut b, &req.title, req.priority.as_deref(), &tags, &today());
    save(&root, &b)?;
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Task {} added to Backlog: {}",
        id, req.title
    ))]))
}

/// Logic for board_move_task tool.
pub fn board_move_task(
    project: &Project,
    req: &BoardMoveTaskRequest,
) -> Result<CallToolResult, McpError> {
    let (mut b, root) = load(project)?;
    board::move_task(&mut b, &req.id, &req.column)
        .map_err(|e| McpError::invalid_params(e, None))?;
    save(&root, &b)?;
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Task {} moved to {}",
        req.id.to_uppercase(),
        req.column
    ))]))
}

/// Logic for sync_todos tool. Scans the whole tree, so callers run it on
/// a blocking thread.
pub fn sync_todos(project: &Project, clean: bool) -> Result<CallToolResult, McpError> {
    let report = void_stack_core::todosync::sync_todos_with(project, clean)
        .map_err(|e| McpError::internal_error(e, None))?;
    let mut out = format!(
        "todo-sync: {} marker(s) in code — {} added, {} unchanged, {} resolved, {} purged",
        report.markers_found, report.added, report.unchanged, report.resolved, report.purged
    );
    if !report.added_ids.is_empty() {
        out.push_str(&format!("\nnew tasks: {}", report.added_ids.join(", ")));
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Logic for board_archive_done tool.
pub fn board_archive_done(
    project: &Project,
    days: Option<i64>,
) -> Result<CallToolResult, McpError> {
    let (mut b, root) = load(project)?;
    let n = board::archive_done(
        &root,
        &mut b,
        days.unwrap_or(14),
        chrono::Local::now().date_naive(),
    )
    .map_err(|e| McpError::internal_error(e, None))?;
    save(&root, &b)?;
    Ok(CallToolResult::success(vec![Content::text(format!(
        "{} task(s) archived to {}",
        n,
        board::ARCHIVE_FILE
    ))]))
}

fn history_status(h: &void_stack_core::boardhistory::TaskHistory) -> String {
    match (&h.current_column, h.archived) {
        (Some(col), _) => col.clone(),
        (None, true) => "archived".into(),
        (None, false) => "removed".into(),
    }
}

fn history_markdown(h: &void_stack_core::boardhistory::TaskHistory) -> String {
    let mut out = format!("## {} — {} [{}]\n", h.id, h.title, history_status(h));
    if let Some(p) = &h.priority {
        out.push_str(&format!("- priority: {}\n", p));
    }
    if !h.tags.is_empty() {
        out.push_str(&format!(
            "- tags: {}\n",
            h.tags
                .iter()
                .map(|t| format!("#{}", t))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if let Some(d) = &h.date {
        out.push_str(&format!("- created: {}\n", d));
    }
    for link in &h.links {
        out.push_str(&format!("- link: {}\n", link));
    }
    if !h.events.is_empty() {
        out.push_str("- timeline:\n");
        for e in &h.events {
            let when = if e.date.is_empty() {
                String::new()
            } else {
                format!(" {}", e.date)
            };
            out.push_str(&format!("  - {}{} → {}\n", e.commit, when, e.column));
        }
    }
    out
}

/// Logic for board_history tool. Walks the git log of BOARD.md, so
/// callers run it on a blocking thread.
pub fn board_history(
    project: &Project,
    req: &BoardHistoryRequest,
) -> Result<CallToolResult, McpError> {
    let root = root_of(project);
    let out = match req.id.as_deref() {
        Some(id) => {
            let h = void_stack_core::boardhistory::task_history(&root, &project.name, id)
                .map_err(|e| McpError::invalid_params(e, None))?;
            history_markdown(&h)
        }
        None => {
            let hist = void_stack_core::boardhistory::board_history(&root, &project.name)
                .map_err(|e| McpError::internal_error(e, None))?;
            let mut out = format!(
                "# Board history — {} ({} task(s) ever)\n\n",
                project.name,
                hist.len()
            );
            for h in &hist {
                out.push_str(&history_markdown(h));
                out.push('\n');
            }
            out
        }
    };
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Logic for board_link_task tool. With the vector feature the query is
/// resolved through the semantic index to concrete files; without it (or
/// when the query already looks like a path/symbol) it is linked verbatim.
pub fn board_link_task(
    project: &Project,
    req: &BoardLinkTaskRequest,
) -> Result<CallToolResult, McpError> {
    let (mut b, root) = load(project)?;

    let mut links: Vec<String> = Vec::new();
    let mut resolved_via_index = false;

    // Path-like or symbol-like queries link directly, no search needed.
    let literal = req.query.contains('/') || req.query.contains("::") || req.query.contains('.');

    #[cfg(feature = "vector")]
    if !literal
        && let Ok(results) = void_stack_core::vector_index::semantic_search(project, &req.query, 3)
    {
        for r in &results {
            if !links.contains(&r.file_path) {
                links.push(r.file_path.clone());
            }
        }
        resolved_via_index = !links.is_empty();
    }

    if links.is_empty() {
        links.push(req.query.trim().to_string());
    }

    board::link_task(&mut b, &req.id, &links).map_err(|e| McpError::invalid_params(e, None))?;
    save(&root, &b)?;

    let how = if resolved_via_index {
        "resolved via semantic index"
    } else {
        "linked verbatim"
    };
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Task {} linked ({}): {}",
        req.id.to_uppercase(),
        how,
        links.join(", ")
    ))]))
}
