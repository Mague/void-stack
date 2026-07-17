//! `void commit` — conventional commit from the current diff.

use anyhow::Result;
use void_stack_core::commitmsg;
use void_stack_core::global_config::{find_project, load_global_config};

pub fn cmd_commit(project_name: &str, dry_run: bool) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();

    let suggestion = commitmsg::suggest_commit_message(&project).map_err(|e| anyhow::anyhow!(e))?;

    if dry_run {
        println!("{}", suggestion.message);
        if !suggestion.resolves.is_empty() {
            eprintln!(
                "(would move to Done: {} — run without --dry-run to commit)",
                suggestion.resolves.join(", ")
            );
        }
        return Ok(());
    }

    let line = commitmsg::perform_commit(&project, &suggestion).map_err(|e| anyhow::anyhow!(e))?;
    println!("✓ {}", line);
    if !suggestion.resolves.is_empty() {
        println!("✓ moved to Done: {}", suggestion.resolves.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil::{config_lock, git, git_repo, register_project, unique_name};

    /// Build a git-repo fixture with one committed file, then dirty it so
    /// the working tree has something to commit. Returns (tempdir, name).
    fn dirty_repo_fixture() -> (tempfile::TempDir, String) {
        let (tmp, root) = git_repo();
        std::fs::write(root.join("app.rs"), "fn main() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "base"]);
        // Dirty a tracked file.
        std::fs::write(root.join("app.rs"), "fn main() { let _ = 1; }\n").unwrap();

        let name = unique_name("commit");
        register_project(&name, &root);
        (tmp, name)
    }

    #[test]
    fn test_cmd_commit_not_found() {
        let _guard = config_lock();
        let err = cmd_commit("no-such-project-xyz", true).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn test_cmd_commit_dry_run_does_not_commit() {
        let _guard = config_lock();
        let (tmp, name) = dirty_repo_fixture();

        // Dry-run only prints the suggestion; the tree stays dirty.
        cmd_commit(&name, true).unwrap();

        let out = std::process::Command::new("git")
            .args(["-C", &tmp.path().to_string_lossy(), "status", "--porcelain"])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&out.stdout).trim().is_empty(),
            "dry-run must leave the working tree dirty"
        );
    }

    #[test]
    fn test_cmd_commit_real_commit_cleans_tree() {
        let _guard = config_lock();
        let (tmp, name) = dirty_repo_fixture();

        cmd_commit(&name, false).unwrap();

        // Tracked changes are committed; -uno ignores scratch dirs the
        // structural graph may create.
        let out = std::process::Command::new("git")
            .args([
                "-C",
                &tmp.path().to_string_lossy(),
                "status",
                "--porcelain",
                "-uno",
            ])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&out.stdout).trim().is_empty(),
            "real commit must clean tracked changes"
        );
    }
}
