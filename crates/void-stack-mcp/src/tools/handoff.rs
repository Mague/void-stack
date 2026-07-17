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
            name: "handoff-wrapper".to_string(),
            description: String::new(),
            path: root.to_string_lossy().to_string(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    /// The wrapper generates the handoff, saves it under the project root,
    /// and returns the markdown plus the saved-path footer.
    #[tokio::test]
    async fn test_session_handoff_wrapper_saves_and_returns() {
        let tmp = tempfile::tempdir().unwrap();
        let project = git_project(tmp.path());
        let out = session_handoff(project, Some("stopping mid-refactor".to_string()))
            .await
            .unwrap();
        let text = out.content[0].as_text().unwrap().text.clone();
        assert!(text.contains("Handoff — handoff-wrapper"));
        assert!(text.contains("_(saved to"));
    }
}
