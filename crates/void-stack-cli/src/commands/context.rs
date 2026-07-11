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
