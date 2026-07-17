//! `void context` — one-call session bootstrap markdown for a project.

use anyhow::Result;
use void_stack_core::global_config::{find_project, load_global_config};

pub fn cmd_context(project_name: &str) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();
    let md = void_stack_core::context::session_context(&project).map_err(|e| anyhow::anyhow!(e))?;
    println!("{}", md);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil;

    #[test]
    fn test_context_unknown_project_errors() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();
        let err = cmd_context("cli-no-such-project-xyz").unwrap_err();
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    /// Every section of the context degrades gracefully (no index, no
    /// graph, tiny repo) — the command must still succeed end to end.
    #[test]
    fn test_context_succeeds_on_minimal_git_fixture() {
        let _guard = testutil::config_lock();
        let (_tmp, root) = testutil::git_repo();
        std::fs::write(root.join("README.md"), "# Demo\n\nA tiny fixture.\n").unwrap();
        testutil::git(&root, &["add", "README.md"]);
        testutil::git(&root, &["commit", "-q", "-m", "docs: readme"]);

        let name = testutil::unique_name("context");
        testutil::register_project(&name, &root);

        cmd_context(&name).unwrap();
    }
}
