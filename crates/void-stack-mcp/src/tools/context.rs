//! session_context tool: one-call session bootstrap for LLM agents.

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;

/// Logic for session_context tool. Filesystem + git + SQLite work, so it
/// runs on a blocking thread like review_diff.
pub async fn session_context(project: Project) -> Result<CallToolResult, McpError> {
    let md =
        tokio::task::spawn_blocking(move || void_stack_core::context::session_context(&project))
            .await
            .map_err(|e| McpError::internal_error(format!("context task failed: {}", e), None))?
            .map_err(|e| McpError::internal_error(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(md)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .expect("git runs");
        assert!(st.status.success(), "git {:?}: {:?}", args, st);
    }

    fn git_project(root: &Path) -> Project {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@t.io"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "base"]);
        Project {
            name: "ctx-wrapper".to_string(),
            description: String::new(),
            path: root.to_string_lossy().to_string(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    /// The async wrapper runs the blocking core builder and returns its
    /// markdown as a successful tool result.
    #[tokio::test]
    async fn test_session_context_wrapper_returns_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let project = git_project(tmp.path());
        let out = session_context(project).await.unwrap();
        let text = out.content[0].as_text().unwrap().text.clone();
        assert!(text.contains("# Session context — ctx-wrapper"));
    }
}
