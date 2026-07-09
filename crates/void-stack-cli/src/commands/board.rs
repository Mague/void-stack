//! `void board` — kanban board stored as BOARD.md in the managed repo.

use std::path::PathBuf;

use anyhow::Result;
use void_stack_core::board;
use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

fn resolve(project_name: &str) -> Result<(Project, PathBuf)> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();
    let root = PathBuf::from(strip_win_prefix(&project.path));
    Ok((project, root))
}

fn load(project_name: &str) -> Result<(board::Board, PathBuf)> {
    let (project, root) = resolve(project_name)?;
    let b = board::load_board(&root, &project.name).map_err(|e| anyhow::anyhow!(e))?;
    Ok((b, root))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn cmd_board_list(project_name: &str) -> Result<()> {
    let (b, root) = load(project_name)?;
    println!(
        "Board — {} ({})\n",
        b.project,
        board::board_path(&root).display()
    );
    for col in &b.columns {
        println!("{} ({})", col.name, col.tasks.len());
        for t in &col.tasks {
            let prio = t
                .priority
                .as_deref()
                .map(|p| format!(" [{}]", p))
                .unwrap_or_default();
            let tags = if t.tags.is_empty() {
                String::new()
            } else {
                format!(
                    "  {}",
                    t.tags
                        .iter()
                        .map(|t| format!("#{}", t))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            let date = t
                .date
                .as_deref()
                .map(|d| format!("  ({})", d))
                .unwrap_or_default();
            println!("  {}{} {}{}{}", t.id, prio, t.title, tags, date);
            if !t.links.is_empty() {
                println!("        ↳ {}", t.links.join(", "));
            }
        }
    }
    Ok(())
}

pub fn cmd_board_add(
    project_name: &str,
    title: &str,
    prio: Option<&str>,
    tags: &[String],
) -> Result<()> {
    let (mut b, root) = load(project_name)?;
    let id = board::add_task(&mut b, title, prio, tags, &today());
    board::save_board(&root, &b).map_err(|e| anyhow::anyhow!(e))?;
    println!("✓ {} added to Backlog: {}", id, title);
    Ok(())
}

pub fn cmd_board_move(project_name: &str, id: &str, column: &str) -> Result<()> {
    let (mut b, root) = load(project_name)?;
    board::move_task(&mut b, id, column).map_err(|e| anyhow::anyhow!(e))?;
    board::save_board(&root, &b).map_err(|e| anyhow::anyhow!(e))?;
    println!("✓ {} moved to {}", id.to_uppercase(), column);
    Ok(())
}

pub fn cmd_board_link(project_name: &str, id: &str, links: &[String]) -> Result<()> {
    let (mut b, root) = load(project_name)?;
    board::link_task(&mut b, id, links).map_err(|e| anyhow::anyhow!(e))?;
    board::save_board(&root, &b).map_err(|e| anyhow::anyhow!(e))?;
    println!("✓ {} linked: {}", id.to_uppercase(), links.join(", "));
    Ok(())
}

pub fn cmd_board_archive(project_name: &str, days: i64) -> Result<()> {
    let (mut b, root) = load(project_name)?;
    let n = board::archive_done(&root, &mut b, days, chrono::Local::now().date_naive())
        .map_err(|e| anyhow::anyhow!(e))?;
    board::save_board(&root, &b).map_err(|e| anyhow::anyhow!(e))?;
    println!("✓ {} task(s) archived to {}", n, board::ARCHIVE_FILE);
    Ok(())
}
