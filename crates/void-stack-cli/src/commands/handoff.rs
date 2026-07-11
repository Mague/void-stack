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
