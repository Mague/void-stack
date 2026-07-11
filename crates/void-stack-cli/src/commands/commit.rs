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
