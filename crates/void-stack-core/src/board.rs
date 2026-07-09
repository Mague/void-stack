//! Git-versioned kanban board stored as plain markdown (`BOARD.md`).
//!
//! The board travels with the repo so it stays in sync across machines via
//! git: one H2 section per column, one list item per task with inline
//! metadata (short id, priority, tags, date) and optional `- link:`
//! sub-bullets attaching files or symbols. Human-readable, mergeable and
//! renderable on GitHub — deliberately NOT a database.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// Default columns for a fresh board, in display order.
pub const DEFAULT_COLUMNS: [&str; 4] = ["Backlog", "Doing", "Review", "Done"];

/// Board file at the project root; `.void/board.md` is the fallback
/// location for repos that don't want a root-level file.
pub const BOARD_FILE: &str = "BOARD.md";
pub const BOARD_FALLBACK: &str = ".void/board.md";
pub const ARCHIVE_FILE: &str = "BOARD_ARCHIVE.md";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BoardTask {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Creation date, `YYYY-MM-DD`.
    #[serde(default)]
    pub date: Option<String>,
    /// Linked files (relative paths) or symbol names.
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BoardColumn {
    pub name: String,
    pub tasks: Vec<BoardTask>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Board {
    pub project: String,
    pub columns: Vec<BoardColumn>,
}

impl Board {
    /// Empty board with the default columns.
    pub fn new(project: &str) -> Self {
        Board {
            project: project.to_string(),
            columns: DEFAULT_COLUMNS
                .iter()
                .map(|n| BoardColumn {
                    name: n.to_string(),
                    tasks: Vec::new(),
                })
                .collect(),
        }
    }

    pub fn find_task(&self, id: &str) -> Option<(&str, &BoardTask)> {
        for col in &self.columns {
            if let Some(t) = col.tasks.iter().find(|t| t.id.eq_ignore_ascii_case(id)) {
                return Some((col.name.as_str(), t));
            }
        }
        None
    }
}

fn task_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^- \*\*([A-Za-z]+-\d+)\*\*\s+(.+)$").unwrap())
}

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s+- link:\s*(.+)$").unwrap())
}

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`([^`]+)`").unwrap())
}

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap())
}

/// Where the board lives for this project. `BOARD.md` at the root wins;
/// `.void/board.md` is used when only it exists (new boards go to the root).
pub fn board_path(project_root: &Path) -> PathBuf {
    let root = project_root.join(BOARD_FILE);
    let fallback = project_root.join(BOARD_FALLBACK);
    if !root.exists() && fallback.exists() {
        fallback
    } else {
        root
    }
}

/// Parse a `BOARD.md`. Every `## Heading` starts a column; prose that is
/// neither a task line nor a link sub-bullet is ignored (the file is
/// canonicalized on the next save).
pub fn parse_board(md: &str, project: &str) -> Board {
    let mut columns: Vec<BoardColumn> = Vec::new();
    for line in md.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            columns.push(BoardColumn {
                name: name.trim().to_string(),
                tasks: Vec::new(),
            });
            continue;
        }
        let Some(col) = columns.last_mut() else {
            continue;
        };
        if let Some(caps) = task_re().captures(line) {
            let id = caps[1].to_string();
            let rest = caps[2].trim();
            let mut priority = None;
            let mut tags = Vec::new();
            let mut date = None;
            for tok in token_re().captures_iter(rest) {
                let tok = tok[1].trim();
                if let Some(p) = tok.strip_prefix("prio:") {
                    priority = Some(p.trim().to_string());
                } else if let Some(tag) = tok.strip_prefix('#') {
                    tags.push(tag.trim().to_string());
                } else if date_re().is_match(tok) {
                    date = Some(tok.to_string());
                }
            }
            let title = token_re().replace_all(rest, "");
            let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
            col.tasks.push(BoardTask {
                id,
                title,
                priority,
                tags,
                date,
                links: Vec::new(),
            });
        } else if let Some(caps) = link_re().captures(line)
            && let Some(task) = col.tasks.last_mut()
        {
            task.links.push(caps[1].trim().to_string());
        }
    }
    // A file with no recognizable columns still yields a usable board.
    if columns.is_empty() {
        return Board::new(project);
    }
    Board {
        project: project.to_string(),
        columns,
    }
}

/// Serialize to the canonical markdown form.
pub fn board_to_markdown(board: &Board) -> String {
    let mut out = format!("# Void Board — {}\n\n", board.project);
    out.push_str(
        "<!-- void-stack board v1 — one \"- **VB-n**\" line per task; \
         \"- link:\" sub-bullets attach files/symbols -->\n",
    );
    for col in &board.columns {
        out.push_str(&format!("\n## {}\n", col.name));
        if !col.tasks.is_empty() {
            out.push('\n');
        }
        for task in &col.tasks {
            out.push_str(&format!("- **{}** {}", task.id, task.title));
            if let Some(p) = &task.priority {
                out.push_str(&format!(" `prio:{}`", p));
            }
            for tag in &task.tags {
                out.push_str(&format!(" `#{}`", tag));
            }
            if let Some(d) = &task.date {
                out.push_str(&format!(" `{}`", d));
            }
            out.push('\n');
            for link in &task.links {
                out.push_str(&format!("  - link: {}\n", link));
            }
        }
    }
    out
}

/// Load the board, or an empty default when the file doesn't exist yet.
pub fn load_board(project_root: &Path, project: &str) -> Result<Board, String> {
    let path = board_path(project_root);
    if !path.exists() {
        return Ok(Board::new(project));
    }
    let md = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    Ok(parse_board(&md, project))
}

pub fn save_board(project_root: &Path, board: &Board) -> Result<(), String> {
    let path = board_path(project_root);
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    }
    std::fs::write(&path, board_to_markdown(board))
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))
}

/// Next short id: `VB-<max+1>` over every column (archive ids never clash
/// because archived tasks keep their id and the max only grows).
pub fn next_id(board: &Board) -> String {
    let max = board
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .filter_map(|t| t.id.rsplit('-').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("VB-{}", max + 1)
}

/// Add a task to Backlog (first column when there is no Backlog). Returns
/// the new id.
pub fn add_task(
    board: &mut Board,
    title: &str,
    priority: Option<&str>,
    tags: &[String],
    date: &str,
) -> String {
    let id = next_id(board);
    let task = BoardTask {
        id: id.clone(),
        title: title.trim().to_string(),
        priority: priority.map(|p| p.to_string()),
        tags: tags.to_vec(),
        date: Some(date.to_string()),
        links: Vec::new(),
    };
    let idx = board
        .columns
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case("Backlog"))
        .or(if board.columns.is_empty() {
            None
        } else {
            Some(0)
        });
    match idx {
        Some(i) => board.columns[i].tasks.push(task),
        None => board.columns.push(BoardColumn {
            name: "Backlog".into(),
            tasks: vec![task],
        }),
    }
    id
}

/// Move a task to another column (case-insensitive), keeping its data.
pub fn move_task(board: &mut Board, id: &str, column: &str) -> Result<(), String> {
    if !board
        .columns
        .iter()
        .any(|c| c.name.eq_ignore_ascii_case(column))
    {
        return Err(format!(
            "unknown column '{}' (available: {})",
            column,
            board
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut task = None;
    for col in &mut board.columns {
        if let Some(pos) = col.tasks.iter().position(|t| t.id.eq_ignore_ascii_case(id)) {
            task = Some(col.tasks.remove(pos));
            break;
        }
    }
    let task = task.ok_or_else(|| format!("task '{}' not found", id))?;
    board
        .columns
        .iter_mut()
        .find(|c| c.name.eq_ignore_ascii_case(column))
        .expect("column checked above")
        .tasks
        .push(task);
    Ok(())
}

/// Edit title/priority/tags in place. `None` keeps the current value.
pub fn edit_task(
    board: &mut Board,
    id: &str,
    title: Option<&str>,
    priority: Option<&str>,
    tags: Option<&[String]>,
) -> Result<(), String> {
    let task = board
        .columns
        .iter_mut()
        .flat_map(|c| &mut c.tasks)
        .find(|t| t.id.eq_ignore_ascii_case(id))
        .ok_or_else(|| format!("task '{}' not found", id))?;
    if let Some(t) = title {
        task.title = t.trim().to_string();
    }
    if let Some(p) = priority {
        task.priority = Some(p.to_string());
    }
    if let Some(tags) = tags {
        task.tags = tags.to_vec();
    }
    Ok(())
}

/// Attach file/symbol links to a task (deduplicated).
pub fn link_task(board: &mut Board, id: &str, links: &[String]) -> Result<(), String> {
    let task = board
        .columns
        .iter_mut()
        .flat_map(|c| &mut c.tasks)
        .find(|t| t.id.eq_ignore_ascii_case(id))
        .ok_or_else(|| format!("task '{}' not found", id))?;
    for link in links {
        let link = link.trim();
        if !link.is_empty() && !task.links.iter().any(|l| l == link) {
            task.links.push(link.to_string());
        }
    }
    Ok(())
}

/// Move Done tasks older than `older_than_days` (by their date; undated
/// tasks are kept) into `BOARD_ARCHIVE.md`, appending under a dated
/// heading. Returns how many tasks were archived. Saves neither file's
/// board side — callers persist the board with `save_board`.
pub fn archive_done(
    project_root: &Path,
    board: &mut Board,
    older_than_days: i64,
    today: chrono::NaiveDate,
) -> Result<usize, String> {
    let cutoff = today - chrono::Duration::days(older_than_days);
    let Some(done) = board
        .columns
        .iter_mut()
        .find(|c| c.name.eq_ignore_ascii_case("Done"))
    else {
        return Ok(0);
    };
    let mut archived = Vec::new();
    done.tasks.retain(|t| {
        let old = t
            .date
            .as_deref()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .map(|d| d <= cutoff)
            .unwrap_or(false);
        if old {
            archived.push(t.clone());
        }
        !old
    });
    if archived.is_empty() {
        return Ok(0);
    }
    let path = project_root.join(ARCHIVE_FILE);
    let mut out = if path.exists() {
        std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?
    } else {
        format!("# Void Board Archive — {}\n", board.project)
    };
    out.push_str(&format!("\n## Archived {}\n\n", today.format("%Y-%m-%d")));
    for task in &archived {
        out.push_str(&format!("- **{}** {}", task.id, task.title));
        if let Some(d) = &task.date {
            out.push_str(&format!(" `{}`", d));
        }
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
    Ok(archived.len())
}

/// Open (non-Done) tasks whose links match any changed file or symbol.
/// File links match by path suffix in either direction (`src/auth/mod.rs`
/// matches a link `auth/mod.rs` and vice versa); symbol links match by
/// exact name, case-sensitive.
pub fn tasks_touching<'a>(
    board: &'a Board,
    files: &[String],
    symbols: &[String],
) -> Vec<(&'a str, &'a BoardTask)> {
    let norm = |s: &str| s.replace('\\', "/");
    let mut hits = Vec::new();
    for col in &board.columns {
        if col.name.eq_ignore_ascii_case("Done") {
            continue;
        }
        for task in &col.tasks {
            let matched = task.links.iter().any(|link| {
                let l = norm(link);
                files.iter().any(|f| {
                    let f = norm(f);
                    f == l || f.ends_with(&format!("/{}", l)) || l.ends_with(&format!("/{}", f))
                }) || symbols
                    .iter()
                    .any(|s| s == &l || s.rsplit("::").next() == Some(l.as_str()))
            });
            if matched {
                hits.push((col.name.as_str(), task));
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_board() -> Board {
        let mut board = Board::new("demo");
        let id = add_task(
            &mut board,
            "Support OAuth login",
            Some("high"),
            &["auth".into(), "backend".into()],
            "2026-07-09",
        );
        link_task(
            &mut board,
            &id,
            &["src/auth/mod.rs".into(), "AuthService::login".into()],
        )
        .unwrap();
        add_task(&mut board, "Write docs", None, &[], "2026-07-08");
        board
    }

    #[test]
    fn test_board_roundtrip() {
        let board = sample_board();
        let md = board_to_markdown(&board);
        let parsed = parse_board(&md, "demo");
        assert_eq!(board, parsed);
    }

    #[test]
    fn test_parse_extracts_metadata() {
        let md = "# Void Board — x\n\n## Backlog\n\n- **VB-7** Fix login `prio:high` `#auth` `2026-07-01`\n  - link: src/a.rs\n";
        let board = parse_board(md, "x");
        let task = &board.columns[0].tasks[0];
        assert_eq!(task.id, "VB-7");
        assert_eq!(task.title, "Fix login");
        assert_eq!(task.priority.as_deref(), Some("high"));
        assert_eq!(task.tags, vec!["auth"]);
        assert_eq!(task.date.as_deref(), Some("2026-07-01"));
        assert_eq!(task.links, vec!["src/a.rs"]);
    }

    #[test]
    fn test_parse_tolerates_extra_prose() {
        let md = "# Title\nsome intro prose\n\n## Backlog\nfree text here\n- not a task bullet\n- **VB-1** Real task\nmore prose\n\n## Done\n";
        let board = parse_board(md, "x");
        assert_eq!(board.columns.len(), 2);
        assert_eq!(board.columns[0].tasks.len(), 1);
        assert_eq!(board.columns[0].tasks[0].title, "Real task");
    }

    #[test]
    fn test_add_task_next_id() {
        let mut board = sample_board();
        assert_eq!(next_id(&board), "VB-3");
        let id = add_task(&mut board, "third", None, &[], "2026-07-09");
        assert_eq!(id, "VB-3");
        // Ids keep growing even when tasks move around.
        move_task(&mut board, "VB-3", "Done").unwrap();
        assert_eq!(next_id(&board), "VB-4");
    }

    #[test]
    fn test_move_task() {
        let mut board = sample_board();
        move_task(&mut board, "vb-1", "Doing").unwrap();
        assert_eq!(board.find_task("VB-1").unwrap().0, "Doing");
        assert!(move_task(&mut board, "VB-1", "Nope").is_err());
        assert!(move_task(&mut board, "VB-99", "Done").is_err());
    }

    #[test]
    fn test_edit_and_link_dedup() {
        let mut board = sample_board();
        edit_task(
            &mut board,
            "VB-2",
            Some("Write better docs"),
            Some("low"),
            None,
        )
        .unwrap();
        let (_, t) = board.find_task("VB-2").unwrap();
        assert_eq!(t.title, "Write better docs");
        assert_eq!(t.priority.as_deref(), Some("low"));
        link_task(&mut board, "VB-1", &["src/auth/mod.rs".into()]).unwrap();
        assert_eq!(board.find_task("VB-1").unwrap().1.links.len(), 2);
    }

    #[test]
    fn test_load_save_and_fallback_path() {
        let tmp = tempfile::tempdir().unwrap();
        // Missing file → default board.
        let board = load_board(tmp.path(), "demo").unwrap();
        assert_eq!(board.columns.len(), 4);
        // Fallback wins only when the root file is absent.
        std::fs::create_dir_all(tmp.path().join(".void")).unwrap();
        std::fs::write(tmp.path().join(".void/board.md"), "## Backlog\n").unwrap();
        assert!(board_path(tmp.path()).ends_with(".void/board.md"));
        std::fs::write(tmp.path().join("BOARD.md"), "## Backlog\n").unwrap();
        assert!(board_path(tmp.path()).ends_with("BOARD.md"));
        // Roundtrip through disk.
        let mut board = sample_board();
        save_board(tmp.path(), &board).unwrap();
        let loaded = load_board(tmp.path(), "demo").unwrap();
        board.project = "demo".into();
        assert_eq!(board, loaded);
    }

    #[test]
    fn test_archive_done_appends_and_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut board = sample_board();
        move_task(&mut board, "VB-2", "Done").unwrap();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 30).unwrap();
        let n = archive_done(tmp.path(), &mut board, 14, today).unwrap();
        assert_eq!(n, 1);
        assert!(board.find_task("VB-2").is_none());
        let archive = std::fs::read_to_string(tmp.path().join(ARCHIVE_FILE)).unwrap();
        assert!(archive.contains("VB-2"));
        assert!(archive.contains("## Archived 2026-07-30"));
        // Recent Done tasks stay.
        move_task(&mut board, "VB-1", "Done").unwrap();
        let n = archive_done(tmp.path(), &mut board, 60, today).unwrap();
        assert_eq!(n, 0);
        assert!(board.find_task("VB-1").is_some());
    }

    #[test]
    fn test_tasks_touching_suffix_and_symbol_match() {
        let board = sample_board();
        let hits = tasks_touching(&board, &["crates/x/src/auth/mod.rs".into()], &[]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.id, "VB-1");
        let hits = tasks_touching(&board, &[], &["AuthService::login".into()]);
        assert_eq!(hits.len(), 1);
        // Done tasks never match.
        let mut board = sample_board();
        move_task(&mut board, "VB-1", "Done").unwrap();
        assert!(tasks_touching(&board, &["src/auth/mod.rs".into()], &[]).is_empty());
    }
}
