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

pub fn cmd_todo_sync(project_name: &str, clean: bool) -> Result<()> {
    let (project, _) = resolve(project_name)?;
    let report = void_stack_core::todosync::sync_todos_with(&project, clean)
        .map_err(|e| anyhow::anyhow!(e))?;
    println!(
        "✓ todo-sync: {} marker(s) in code — {} added, {} unchanged, {} resolved, {} purged",
        report.markers_found, report.added, report.unchanged, report.resolved, report.purged
    );
    if !report.added_ids.is_empty() {
        println!("  new tasks: {}", report.added_ids.join(", "));
    }
    Ok(())
}

fn status_of(h: &void_stack_core::boardhistory::TaskHistory) -> String {
    match (&h.current_column, h.archived) {
        (Some(col), _) => col.clone(),
        (None, true) => "archived".into(),
        (None, false) => "removed".into(),
    }
}

pub fn cmd_board_history(project_name: &str, json: bool) -> Result<()> {
    let (project, root) = resolve(project_name)?;
    let hist = void_stack_core::boardhistory::board_history(&root, &project.name)
        .map_err(|e| anyhow::anyhow!(e))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&hist)?);
        return Ok(());
    }
    println!(
        "Board history — {} ({} task(s) ever)\n",
        project.name,
        hist.len()
    );
    for h in &hist {
        let trail = h
            .events
            .iter()
            .map(|e| e.column.as_str())
            .collect::<Vec<_>>()
            .join(" → ");
        println!("  {} [{}] {}", h.id, status_of(h), h.title);
        println!("        {}", trail);
    }
    Ok(())
}

pub fn cmd_board_show(project_name: &str, id: &str, json: bool) -> Result<()> {
    let (project, root) = resolve(project_name)?;
    let h = void_stack_core::boardhistory::task_history(&root, &project.name, id)
        .map_err(|e| anyhow::anyhow!(e))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&h)?);
        return Ok(());
    }
    println!("{} — {}", h.id, h.title);
    println!("  status:   {}", status_of(&h));
    if let Some(p) = &h.priority {
        println!("  priority: {}", p);
    }
    if !h.tags.is_empty() {
        println!(
            "  tags:     {}",
            h.tags
                .iter()
                .map(|t| format!("#{}", t))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    if let Some(d) = &h.date {
        println!("  created:  {}", d);
    }
    for link in &h.links {
        println!("  link:     {}", link);
    }
    if !h.events.is_empty() {
        println!("  timeline:");
        for e in &h.events {
            let when = if e.date.is_empty() {
                String::new()
            } else {
                format!("  {}", e.date)
            };
            let who = if e.author.is_empty() {
                String::new()
            } else {
                format!("  ({})", e.author)
            };
            println!("    {:>12}{}  → {}{}", e.commit, when, e.column, who);
        }
    }
    Ok(())
}

pub fn cmd_board_timeline(
    project_name: &str,
    by: &str,
    since: Option<&str>,
    json: bool,
) -> Result<()> {
    let (project, root) = resolve(project_name)?;
    let group = void_stack_core::timeline::GroupBy::parse(by).map_err(|e| anyhow::anyhow!(e))?;
    let buckets = void_stack_core::timeline::board_timeline(&root, &project.name, group, since)
        .map_err(|e| anyhow::anyhow!(e))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&buckets)?);
        return Ok(());
    }
    println!("Timeline — {} (by {})\n", project.name, by);
    for b in &buckets {
        println!(
            "## {} — {} commit(s), {} task(s)",
            b.key,
            b.commits.len(),
            b.tasks.len()
        );
        for t in &b.tasks {
            println!("  ◧ {} [{}] {}", t.id, t.column, t.title);
        }
        for c in &b.commits {
            let kind = match (&c.ctype, &c.scope) {
                (Some(t), Some(s)) => format!("{}({})", t, s),
                (Some(t), None) => t.clone(),
                _ => "-".into(),
            };
            let resolves = if c.resolves.is_empty() {
                String::new()
            } else {
                format!("  [{}]", c.resolves.join(", "))
            };
            println!(
                "  {}  {}  {}  {}{}",
                c.commit, c.date, kind, c.subject, resolves
            );
        }
        println!();
    }
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
