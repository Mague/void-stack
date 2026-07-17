//! `void handoff` — session journal saved to .void/journal/.

use std::path::PathBuf;

use anyhow::Result;
use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::handoff;
use void_stack_core::runner::local::strip_win_prefix;

pub fn cmd_handoff(project_name: &str, note: Option<&str>) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();
    let root = PathBuf::from(strip_win_prefix(&project.path));

    let md = handoff::generate_handoff(&project, note).map_err(|e| anyhow::anyhow!(e))?;
    let path =
        handoff::save_handoff(&root, &md, chrono::Local::now()).map_err(|e| anyhow::anyhow!(e))?;

    println!("{}", md);
    eprintln!(
        "✓ saved to {} (LATEST.md updated — commit .void/journal/ to share it)",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil;

    #[test]
    fn test_handoff_unknown_project_errors() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();
        let err = cmd_handoff("cli-no-such-project-xyz", None).unwrap_err();
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    #[test]
    fn test_handoff_writes_journal_and_latest_with_note() {
        let _guard = testutil::config_lock();
        let (_tmp, root) = testutil::git_repo();
        std::fs::write(root.join("a.txt"), "hello\n").unwrap();
        testutil::git(&root, &["add", "a.txt"]);
        testutil::git(&root, &["commit", "-q", "-m", "init"]);
        // Leave an uncommitted change so the diff section has content.
        std::fs::write(root.join("a.txt"), "hello world\n").unwrap();

        let name = testutil::unique_name("handoff");
        testutil::register_project(&name, &root);

        cmd_handoff(&name, Some("wrapping up the auth refactor")).unwrap();

        let latest = root.join(handoff::JOURNAL_DIR).join(handoff::LATEST_FILE);
        assert!(latest.exists(), "LATEST.md must be written");
        let md = std::fs::read_to_string(&latest).unwrap();
        assert!(md.contains("# Handoff —"), "got:\n{md}");
        assert!(md.contains("wrapping up the auth refactor"));
        assert!(md.contains("Uncommitted work"));
    }
}
