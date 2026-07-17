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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil;

    /// Registered fixture project over a fresh git repo. Caller must hold
    /// the config lock for the whole test body.
    fn fixture(area: &str) -> (tempfile::TempDir, PathBuf, String) {
        let (tmp, root) = testutil::git_repo();
        let name = testutil::unique_name(area);
        testutil::register_project(&name, &root);
        (tmp, root, name)
    }

    fn commit_board(root: &std::path::Path, md: &str, msg: &str) {
        std::fs::write(root.join("BOARD.md"), md).unwrap();
        testutil::git(root, &["add", "BOARD.md"]);
        testutil::git(root, &["commit", "-q", "-m", msg]);
    }

    #[test]
    fn test_board_add_creates_board_file_with_task() {
        let _guard = testutil::config_lock();
        let (_tmp, root, name) = fixture("board-add");

        cmd_board_add(&name, "Fix the login flow", Some("high"), &["auth".into()]).unwrap();

        let md = std::fs::read_to_string(root.join("BOARD.md")).unwrap();
        assert!(
            md.contains("VB-1"),
            "board should contain the new id:\n{md}"
        );
        assert!(md.contains("Fix the login flow"));

        let b = board::load_board(&root, &name).unwrap();
        let (col, task) = b.find_task("VB-1").expect("task exists");
        assert_eq!(col, "Backlog");
        assert_eq!(task.priority.as_deref(), Some("high"));
        assert_eq!(task.tags, vec!["auth".to_string()]);
    }

    #[test]
    fn test_board_move_and_done_shortcut() {
        let _guard = testutil::config_lock();
        let (_tmp, root, name) = fixture("board-move");

        cmd_board_add(&name, "Ship it", None, &[]).unwrap();
        cmd_board_move(&name, "vb-1", "Doing").unwrap();
        let b = board::load_board(&root, &name).unwrap();
        assert_eq!(b.find_task("VB-1").unwrap().0, "Doing");

        // `void board done` routes through the same move with "Done".
        cmd_board_move(&name, "VB-1", "Done").unwrap();
        let b = board::load_board(&root, &name).unwrap();
        assert_eq!(b.find_task("VB-1").unwrap().0, "Done");
    }

    #[test]
    fn test_board_move_unknown_task_errors() {
        let _guard = testutil::config_lock();
        let (_tmp, _root, name) = fixture("board-move-bad");
        cmd_board_add(&name, "Only task", None, &[]).unwrap();
        assert!(cmd_board_move(&name, "VB-99", "Doing").is_err());
    }

    #[test]
    fn test_board_link_attaches_links() {
        let _guard = testutil::config_lock();
        let (_tmp, root, name) = fixture("board-link");
        cmd_board_add(&name, "Refactor auth", None, &[]).unwrap();

        cmd_board_link(
            &name,
            "VB-1",
            &["src/auth/mod.rs".into(), "verify_token".into()],
        )
        .unwrap();

        let b = board::load_board(&root, &name).unwrap();
        let (_, task) = b.find_task("VB-1").unwrap();
        assert!(task.links.contains(&"src/auth/mod.rs".to_string()));
        assert!(task.links.contains(&"verify_token".to_string()));
    }

    #[test]
    fn test_board_list_ok_and_unknown_project_errors() {
        let _guard = testutil::config_lock();
        let (_tmp, _root, name) = fixture("board-list");
        cmd_board_add(&name, "Visible task", Some("low"), &["ui".into()]).unwrap();
        cmd_board_list(&name).unwrap();

        let err = cmd_board_list("cli-no-such-project-xyz").unwrap_err();
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    #[test]
    fn test_board_archive_moves_todays_done_tasks_with_zero_days() {
        let _guard = testutil::config_lock();
        let (_tmp, root, name) = fixture("board-archive");
        cmd_board_add(&name, "Old chore", None, &[]).unwrap();
        cmd_board_move(&name, "VB-1", "Done").unwrap();

        // days = 0 → cutoff is today, so a task dated today is archived.
        cmd_board_archive(&name, 0).unwrap();

        let archive = std::fs::read_to_string(root.join(board::ARCHIVE_FILE)).unwrap();
        assert!(archive.contains("VB-1"));
        assert!(archive.contains("Old chore"));
        let b = board::load_board(&root, &name).unwrap();
        assert!(b.find_task("VB-1").is_none(), "task must leave the board");
    }

    #[test]
    fn test_board_archive_keeps_recent_done_tasks() {
        let _guard = testutil::config_lock();
        let (_tmp, root, name) = fixture("board-archive-keep");
        cmd_board_add(&name, "Fresh work", None, &[]).unwrap();
        cmd_board_move(&name, "VB-1", "Done").unwrap();

        // Task is dated today — a 14-day window must keep it.
        cmd_board_archive(&name, 14).unwrap();

        assert!(!root.join(board::ARCHIVE_FILE).exists());
        let b = board::load_board(&root, &name).unwrap();
        assert_eq!(b.find_task("VB-1").unwrap().0, "Done");
    }

    #[test]
    fn test_board_history_and_show_over_committed_board() {
        let _guard = testutil::config_lock();
        let (_tmp, root, name) = fixture("board-history");
        commit_board(
            &root,
            "## Backlog\n\n- **VB-1** First task `prio:high`\n",
            "v1",
        );
        commit_board(
            &root,
            "## Backlog\n\n## Doing\n\n- **VB-1** First task `prio:high`\n",
            "v2",
        );

        // Human and JSON renderings both succeed over real git history.
        cmd_board_history(&name, false).unwrap();
        cmd_board_history(&name, true).unwrap();
        cmd_board_show(&name, "vb-1", false).unwrap();
        cmd_board_show(&name, "VB-1", true).unwrap();

        assert!(cmd_board_show(&name, "VB-99", false).is_err());
    }

    #[test]
    fn test_board_timeline_groups_and_rejects_bad_group() {
        let _guard = testutil::config_lock();
        let (_tmp, root, name) = fixture("board-timeline");
        commit_board(
            &root,
            "## Doing\n\n- **VB-1** Ship the thing\n",
            "feat(core): v1",
        );

        cmd_board_timeline(&name, "month", None, false).unwrap();
        cmd_board_timeline(&name, "week", None, true).unwrap();

        let err = cmd_board_timeline(&name, "fortnight", None, false).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_status_of_maps_column_archive_and_removal() {
        fn hist(
            current: Option<&str>,
            archived: bool,
        ) -> void_stack_core::boardhistory::TaskHistory {
            void_stack_core::boardhistory::TaskHistory {
                id: "VB-1".into(),
                title: "t".into(),
                priority: None,
                tags: vec![],
                date: None,
                links: vec![],
                current_column: current.map(str::to_string),
                archived,
                events: vec![],
            }
        }
        assert_eq!(status_of(&hist(Some("Doing"), false)), "Doing");
        assert_eq!(status_of(&hist(Some("Done"), true)), "Done");
        assert_eq!(status_of(&hist(None, true)), "archived");
        assert_eq!(status_of(&hist(None, false)), "removed");
    }
}
