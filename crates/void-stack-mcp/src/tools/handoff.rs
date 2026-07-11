//! session_handoff tool: session journal for context transfer.

use std::path::PathBuf;

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

/// Logic for session_handoff tool. Git + graph work → blocking thread.
pub async fn session_handoff(
    project: Project,
    note: Option<String>,
) -> Result<CallToolResult, McpError> {
    let out = tokio::task::spawn_blocking(move || {
        let root = PathBuf::from(strip_win_prefix(&project.path));
        let md = void_stack_core::handoff::generate_handoff(&project, note.as_deref())?;
        let path = void_stack_core::handoff::save_handoff(&root, &md, chrono::Local::now())?;
        Ok::<String, String>(format!("{}\n\n_(saved to {})_\n", md, path.display()))
    })
    .await
    .map_err(|e| McpError::internal_error(format!("handoff task failed: {}", e), None))?
    .map_err(|e| McpError::internal_error(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(out)]))
}
