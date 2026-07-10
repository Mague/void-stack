//! `void todo-sync`: mirror code markers (TODO/FIXME/HACK) into BOARD.md.
//!
//! Scans the project for `// TODO(name): ...`, `// FIXME: ...` and
//! `// HACK: ...` comments (reusing the explicit-debt scanner) and syncs
//! them as Backlog tasks with an automatic link to the file and, when the
//! structural graph is available, the containing symbol. Idempotent: each
//! marker gets a stable content hash stored on the task (`sync:<hash>`),
//! so re-runs never duplicate. Markers that disappear from the code mark
//! their task resolved (moved to Done + `auto-resolved` tag) — never a
//! silent delete.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::analyzer::explicit_debt::{ExplicitDebtItem, scan_explicit_debt};
use crate::board::{self, Board};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Marker kinds that become board tasks (the rest of the debt keywords
/// stay analysis-only noise).
const SYNC_KINDS: [&str; 3] = ["TODO", "FIXME", "HACK"];
const MAX_TITLE: usize = 90;

#[derive(Debug, Clone, serde::Serialize)]
pub struct TodoSyncReport {
    pub markers_found: usize,
    pub added: usize,
    pub unchanged: usize,
    pub resolved: usize,
    pub added_ids: Vec<String>,
}

/// Stable idempotence key: kind + file + normalized text (line numbers
/// churn on every edit, so they stay out of the hash).
fn marker_hash(item: &ExplicitDebtItem) -> String {
    let mut hasher = Sha256::new();
    hasher.update(item.kind.as_bytes());
    hasher.update(b"|");
    hasher.update(item.file.as_bytes());
    hasher.update(b"|");
    hasher.update(item.text.trim().as_bytes());
    let digest = hasher.finalize();
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3]
    )
}

/// Extract "(name)" assignee from marker text like "(edu): fix later".
/// Returns (assignee, remaining text).
fn split_assignee(text: &str) -> (Option<String>, String) {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix('(')
        && let Some(close) = rest.find(')')
    {
        let name = rest[..close].trim();
        let after = rest[close + 1..].trim_start_matches([':', ' ', '-']).trim();
        if !name.is_empty() && name.len() <= 30 {
            return (Some(name.to_string()), after.to_string());
        }
    }
    (None, trimmed.to_string())
}

fn priority_for(kind: &str) -> Option<&'static str> {
    match kind {
        "FIXME" => Some("high"),
        "HACK" => Some("medium"),
        _ => None,
    }
}

/// Resolve the symbol containing `file:line` via the structural graph:
/// the tightest node whose span covers the line.
#[cfg(feature = "structural")]
fn containing_symbol(project: &Project, file: &str, line: usize) -> Option<String> {
    let conn = crate::structural::open_db(project).ok()?;
    conn.query_row(
        "SELECT qualified_name FROM nodes
         WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2
         ORDER BY (line_end - line_start) ASC LIMIT 1",
        rusqlite::params![file, line as i64],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

#[cfg(not(feature = "structural"))]
fn containing_symbol(_project: &Project, _file: &str, _line: usize) -> Option<String> {
    None
}

/// Scan the project and sync markers into the board. Loads and saves
/// BOARD.md itself; returns what changed.
pub fn sync_todos(project: &Project) -> Result<TodoSyncReport, String> {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    let mut board = board::load_board(&root, &project.name)?;
    let report = sync_into_board(project, &root, &mut board);
    if report.added > 0 || report.resolved > 0 {
        board::save_board(&root, &board)?;
    }
    Ok(report)
}

/// Core sync on an in-memory board (unit-testable without saving).
pub fn sync_into_board(project: &Project, root: &Path, board: &mut Board) -> TodoSyncReport {
    let markers: Vec<ExplicitDebtItem> = scan_explicit_debt(root)
        .into_iter()
        .filter(|i| SYNC_KINDS.contains(&i.kind.as_str()))
        .collect();

    // hash → marker (first occurrence wins on duplicates).
    let mut by_hash: HashMap<String, &ExplicitDebtItem> = HashMap::new();
    for m in &markers {
        by_hash.entry(marker_hash(m)).or_insert(m);
    }

    // Existing synced hashes on the board.
    let existing: HashMap<String, String> = board
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .filter_map(|t| t.sync.clone().map(|h| (h, t.id.clone())))
        .collect();

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut added_ids = Vec::new();
    let mut unchanged = 0usize;

    for (hash, marker) in &by_hash {
        if existing.contains_key(hash) {
            unchanged += 1;
            continue;
        }
        let (assignee, text) = split_assignee(&marker.text);
        let mut title = if text.is_empty() {
            format!("{} in {}", marker.kind, marker.file)
        } else {
            text
        };
        if title.len() > MAX_TITLE {
            let mut cut = MAX_TITLE;
            while !title.is_char_boundary(cut) {
                cut -= 1;
            }
            title.truncate(cut);
            title.push('…');
        }
        let mut tags = vec![marker.kind.to_lowercase()];
        if let Some(name) = assignee {
            tags.push(name);
        }
        let id = board::add_task(board, &title, priority_for(&marker.kind), &tags, &today);
        let mut links = vec![marker.file.clone()];
        if let Some(sym) = containing_symbol(project, &marker.file, marker.line) {
            links.push(sym);
        }
        let _ = board::link_task(board, &id, &links);
        if let Some(task) = board
            .columns
            .iter_mut()
            .flat_map(|c| &mut c.tasks)
            .find(|t| t.id == id)
        {
            task.sync = Some(hash.clone());
        }
        added_ids.push(id);
    }

    // Markers gone from the code: resolve their tasks (Done + tag), keep
    // anything a human already moved to Done untouched.
    let mut resolved = 0usize;
    let orphan_ids: Vec<String> = board
        .columns
        .iter()
        .filter(|c| !c.name.eq_ignore_ascii_case("Done"))
        .flat_map(|c| &c.tasks)
        .filter(|t| {
            t.sync
                .as_ref()
                .map(|h| !by_hash.contains_key(h))
                .unwrap_or(false)
        })
        .map(|t| t.id.clone())
        .collect();
    for id in &orphan_ids {
        if board::move_task(board, id, "Done").is_ok() {
            if let Some(task) = board
                .columns
                .iter_mut()
                .flat_map(|c| &mut c.tasks)
                .find(|t| &t.id == id)
                && !task.tags.iter().any(|t| t == "auto-resolved")
            {
                task.tags.push("auto-resolved".to_string());
            }
            resolved += 1;
        }
    }

    added_ids.sort();
    TodoSyncReport {
        markers_found: by_hash.len(),
        added: added_ids.len(),
        unchanged,
        resolved,
        added_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &Path) -> Project {
        Project {
            name: "todosync-demo".into(),
            path: dir.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    #[test]
    fn test_split_assignee() {
        let (who, text) = split_assignee("(edu): fix the retry loop");
        assert_eq!(who.as_deref(), Some("edu"));
        assert_eq!(text, "fix the retry loop");
        let (who, text) = split_assignee("plain text");
        assert_eq!(who, None);
        assert_eq!(text, "plain text");
    }

    #[test]
    fn test_sync_adds_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.rs"),
            "fn work() {\n    // TODO(edu): retry on timeout\n    // HACK: temporary cast\n}\n",
        )
        .unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);

        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.added, 2, "{:?}", report);
        assert_eq!(report.resolved, 0);
        let backlog = &board.columns[0];
        assert_eq!(backlog.tasks.len(), 2);
        let todo = backlog
            .tasks
            .iter()
            .find(|t| t.tags.contains(&"todo".to_string()))
            .unwrap();
        assert_eq!(todo.title, "retry on timeout");
        assert!(todo.tags.contains(&"edu".to_string()));
        assert!(todo.links.contains(&"a.rs".to_string()));
        assert!(todo.sync.is_some());
        let hack = backlog
            .tasks
            .iter()
            .find(|t| t.tags.contains(&"hack".to_string()))
            .unwrap();
        assert_eq!(hack.priority.as_deref(), Some("medium"));

        // Second run: nothing new.
        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.added, 0, "{:?}", report);
        assert_eq!(report.unchanged, 2);
        assert_eq!(board.columns[0].tasks.len(), 2);
    }

    #[test]
    fn test_gone_marker_resolves_task_not_deletes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "// TODO: short lived\n").unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);
        sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(board.columns[0].tasks.len(), 1);
        let id = board.columns[0].tasks[0].id.clone();

        // Marker removed from the code.
        std::fs::write(dir.path().join("a.rs"), "fn done() {}\n").unwrap();
        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.resolved, 1, "{:?}", report);
        let (col, task) = board.find_task(&id).expect("task must survive");
        assert_eq!(col, "Done");
        assert!(task.tags.contains(&"auto-resolved".to_string()));

        // Re-running stays stable.
        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.resolved, 0);
        assert_eq!(report.added, 0);
    }

    #[test]
    fn test_manual_tasks_untouched() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "// FIXME: broken\n").unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);
        board::add_task(&mut board, "manual task", None, &[], "2026-07-09");
        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.added, 1);
        // Manual task has no sync hash → never resolved by the scanner.
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.resolved, 1);
        let (col, _) = board.find_task("VB-1").unwrap();
        assert_eq!(col, "Backlog", "manual task stays put");
    }

    #[test]
    fn test_sync_hash_roundtrips_through_markdown() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "// TODO: persist me\n").unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);
        sync_into_board(&project, dir.path(), &mut board);
        let md = board::board_to_markdown(&board);
        assert!(md.contains("`sync:"), "{md}");
        let parsed = board::parse_board(&md, &project.name);
        assert_eq!(
            parsed.columns[0].tasks[0].sync,
            board.columns[0].tasks[0].sync
        );
        // And the reparsed board still dedupes.
        let mut parsed = parsed;
        let report = sync_into_board(&project, dir.path(), &mut parsed);
        assert_eq!(report.added, 0);
    }
}
