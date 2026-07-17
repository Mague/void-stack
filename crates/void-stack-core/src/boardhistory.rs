//! Task history reconstructed from the git log of `BOARD.md`.
//!
//! The board is a git-versioned file, so every commit that touched it is a
//! snapshot of the whole board. Walking those snapshots oldest-first gives
//! the full life of every task that EVER existed — including tasks that
//! were archived to `BOARD_ARCHIVE.md` or deleted outright — as a series
//! of column transitions (`Backlog → Doing → Done → archived`).

use std::collections::BTreeMap;
use std::path::Path;

use crate::board::{self, BoardTask};
use crate::git_util;

/// Pseudo-column for a task that left the board in a commit.
pub const REMOVED: &str = "(removed)";
/// Pseudo-commit for the uncommitted working-tree state.
pub const UNCOMMITTED: &str = "(uncommitted)";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TaskEvent {
    /// Short commit hash, or `(uncommitted)` for the working tree.
    pub commit: String,
    /// Committer date, `YYYY-MM-DD` (empty for the working tree).
    pub date: String,
    pub author: String,
    /// Column the task sits in AFTER this commit; `(removed)` when the
    /// commit took it off the board (archived or deleted).
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TaskHistory {
    pub id: String,
    /// Latest known title (current board wins, else last commit seen).
    pub title: String,
    pub priority: Option<String>,
    pub tags: Vec<String>,
    pub date: Option<String>,
    pub links: Vec<String>,
    /// Column on the current board; `None` when the task is gone.
    pub current_column: Option<String>,
    /// The task appears in `BOARD_ARCHIVE.md`.
    pub archived: bool,
    /// Column transitions, oldest first. The first event is the commit
    /// that introduced the task.
    pub events: Vec<TaskEvent>,
}

/// Full board history: one entry per task id ever seen in git (plus the
/// working tree), sorted by numeric id. Repos where the board was never
/// committed degrade to the current board with an empty timeline.
pub fn board_history(project_root: &Path, project: &str) -> Result<Vec<TaskHistory>, String> {
    let mut histories: BTreeMap<String, TaskHistory> = BTreeMap::new();
    // id -> column after the previously processed snapshot.
    let mut last_column: BTreeMap<String, String> = BTreeMap::new();

    for snap in board_snapshots(project_root) {
        let parsed = board::parse_board(&snap.content, project);
        apply_snapshot(
            &mut histories,
            &mut last_column,
            &parsed,
            &snap.commit,
            &snap.date,
            &snap.author,
        );
    }

    // Working tree last: it wins on titles/metadata and closes the
    // timeline with an `(uncommitted)` event when it diverges from HEAD.
    let current = board::load_board(project_root, project)?;
    apply_snapshot(
        &mut histories,
        &mut last_column,
        &current,
        UNCOMMITTED,
        "",
        "",
    );

    for h in histories.values_mut() {
        h.current_column = current.find_task(&h.id).map(|(col, _)| col.to_string());
    }

    // Archive membership: `parse_board` reads the archive fine — its
    // "## Archived YYYY-MM-DD" headings parse as columns.
    let archive_path = project_root.join(board::ARCHIVE_FILE);
    if let Ok(md) = std::fs::read_to_string(&archive_path) {
        let archive = board::parse_board(&md, project);
        for col in &archive.columns {
            for t in &col.tasks {
                if let Some(h) = histories.get_mut(&t.id.to_uppercase()) {
                    h.archived = true;
                }
            }
        }
    }

    let mut out: Vec<TaskHistory> = histories.into_values().collect();
    out.sort_by_key(|h| {
        h.id.rsplit('-')
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });
    Ok(out)
}

/// History of a single task (case-insensitive id).
pub fn task_history(project_root: &Path, project: &str, id: &str) -> Result<TaskHistory, String> {
    board_history(project_root, project)?
        .into_iter()
        .find(|h| h.id.eq_ignore_ascii_case(id))
        .ok_or_else(|| format!("task '{}' not found in the board or its git history", id))
}

struct Snapshot {
    commit: String,
    date: String,
    author: String,
    content: String,
}

/// Every committed version of the board file, oldest first. Both board
/// locations are tracked so a file that moved keeps its history. Errors
/// (not a repo, file never committed) degrade to an empty list.
///
/// All snapshot contents come through one `git cat-file --batch` process
/// (see [`git_util::batch_read_objects`]) — the old one-`git show`-per-
/// commit approach made the board take seconds to load on Windows, where
/// process spawns are expensive.
fn board_snapshots(project_root: &Path) -> Vec<Snapshot> {
    let Ok(log) = git_util::git_output(
        project_root,
        &[
            "log",
            "--reverse",
            "--format=%h%x09%cs%x09%an",
            "--",
            board::BOARD_FILE,
            board::BOARD_FALLBACK,
        ],
    ) else {
        return Vec::new();
    };
    let mut metas: Vec<(String, String, String)> = Vec::new();
    for line in log.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(hash), Some(date), author) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        metas.push((
            hash.to_string(),
            date.to_string(),
            author.unwrap_or("").to_string(),
        ));
    }
    if metas.is_empty() {
        return Vec::new();
    }

    // Two specs per commit: the primary board location and its fallback.
    let mut specs = Vec::with_capacity(metas.len() * 2);
    for (hash, _, _) in &metas {
        specs.push(format!("{}:{}", hash, board::BOARD_FILE));
        specs.push(format!("{}:{}", hash, board::BOARD_FALLBACK));
    }
    let Ok(objects) = git_util::batch_read_objects(project_root, &specs) else {
        return Vec::new();
    };

    let mut snaps = Vec::new();
    for (i, (hash, date, author)) in metas.into_iter().enumerate() {
        // The root-level file wins, matching `board::board_path`.
        let content = objects[i * 2].as_ref().or(objects[i * 2 + 1].as_ref());
        if let Some(bytes) = content {
            snaps.push(Snapshot {
                commit: hash,
                date,
                author,
                content: String::from_utf8_lossy(bytes).to_string(),
            });
        }
    }
    snaps
}

/// Fold one board snapshot into the running histories: new tasks open a
/// timeline, column changes append an event, disappearances close with a
/// `(removed)` event. Titles/metadata always refresh to the latest seen.
fn apply_snapshot(
    histories: &mut BTreeMap<String, TaskHistory>,
    last_column: &mut BTreeMap<String, String>,
    snapshot: &board::Board,
    commit: &str,
    date: &str,
    author: &str,
) {
    let mut seen: BTreeMap<String, (String, BoardTask)> = BTreeMap::new();
    for col in &snapshot.columns {
        for t in &col.tasks {
            seen.insert(t.id.to_uppercase(), (col.name.clone(), t.clone()));
        }
    }

    for (key, (column, task)) in &seen {
        let entry = histories.entry(key.clone()).or_insert_with(|| TaskHistory {
            id: task.id.clone(),
            title: String::new(),
            priority: None,
            tags: Vec::new(),
            date: None,
            links: Vec::new(),
            current_column: None,
            archived: false,
            events: Vec::new(),
        });
        entry.title = task.title.clone();
        entry.priority = task.priority.clone();
        entry.tags = task.tags.clone();
        entry.date = task.date.clone();
        entry.links = task.links.clone();
        if last_column.get(key) != Some(column) {
            entry.events.push(TaskEvent {
                commit: commit.to_string(),
                date: date.to_string(),
                author: author.to_string(),
                column: column.clone(),
            });
            last_column.insert(key.clone(), column.clone());
        }
    }

    // Tasks present before but absent from this snapshot left the board.
    let gone: Vec<String> = last_column
        .iter()
        .filter(|(k, col)| col.as_str() != REMOVED && !seen.contains_key(*k))
        .map(|(k, _)| k.clone())
        .collect();
    for key in gone {
        if let Some(h) = histories.get_mut(&key) {
            h.events.push(TaskEvent {
                commit: commit.to_string(),
                date: date.to_string(),
                author: author.to_string(),
                column: REMOVED.to_string(),
            });
        }
        last_column.insert(key, REMOVED.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn sh_git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            st.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&st.stderr)
        );
    }

    fn repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        sh_git(&root, &["init", "-q"]);
        sh_git(&root, &["config", "user.email", "t@t.io"]);
        sh_git(&root, &["config", "user.name", "t"]);
        sh_git(&root, &["config", "commit.gpgsign", "false"]);
        (tmp, root)
    }

    fn commit_board(root: &Path, md: &str, msg: &str) {
        std::fs::write(root.join("BOARD.md"), md).unwrap();
        sh_git(root, &["add", "BOARD.md"]);
        sh_git(root, &["commit", "-q", "-m", msg]);
    }

    #[test]
    fn test_history_tracks_transitions_and_removal() {
        let (_tmp, root) = repo();
        commit_board(
            &root,
            "## Backlog\n\n- **VB-1** First task `prio:high`\n",
            "v1",
        );
        commit_board(
            &root,
            "## Backlog\n\n- **VB-2** Second\n\n## Doing\n\n- **VB-1** First task `prio:high`\n",
            "v2",
        );
        commit_board(
            &root,
            "## Doing\n\n## Done\n\n- **VB-1** First task `prio:high`\n",
            "v3",
        );

        let hist = board_history(&root, "demo").unwrap();
        assert_eq!(hist.len(), 2);

        let vb1 = &hist[0];
        assert_eq!(vb1.id, "VB-1");
        assert_eq!(vb1.priority.as_deref(), Some("high"));
        let cols: Vec<&str> = vb1.events.iter().map(|e| e.column.as_str()).collect();
        assert_eq!(cols, vec!["Backlog", "Doing", "Done"]);
        assert_eq!(vb1.current_column.as_deref(), Some("Done"));
        assert!(vb1.events.iter().all(|e| e.commit != UNCOMMITTED));
        assert!(vb1.events.iter().all(|e| !e.date.is_empty()));

        // VB-2 was deleted in v3: still in the history, closed by (removed).
        let vb2 = &hist[1];
        assert_eq!(vb2.id, "VB-2");
        let cols: Vec<&str> = vb2.events.iter().map(|e| e.column.as_str()).collect();
        assert_eq!(cols, vec!["Backlog", REMOVED]);
        assert_eq!(vb2.current_column, None);
        assert!(!vb2.archived);
    }

    #[test]
    fn test_history_sees_uncommitted_state_and_archive() {
        let (_tmp, root) = repo();
        commit_board(&root, "## Doing\n\n- **VB-1** Ship it\n", "v1");
        // Uncommitted: VB-1 moved to Done, and an archive file exists
        // holding an older task never committed to BOARD.md history.
        std::fs::write(root.join("BOARD.md"), "## Done\n\n- **VB-1** Ship it\n").unwrap();
        std::fs::write(
            root.join(board::ARCHIVE_FILE),
            "# Void Board Archive — demo\n\n## Archived 2026-07-01\n\n- **VB-1** Ship it\n",
        )
        .unwrap();

        let vb1 = task_history(&root, "demo", "vb-1").unwrap();
        let last = vb1.events.last().unwrap();
        assert_eq!(last.commit, UNCOMMITTED);
        assert_eq!(last.column, "Done");
        assert_eq!(vb1.current_column.as_deref(), Some("Done"));
        assert!(vb1.archived);

        assert!(task_history(&root, "demo", "VB-99").is_err());
    }

    #[test]
    fn test_history_reads_fallback_board_location() {
        // A board that only ever lived at `.void/board.md` must still get
        // its snapshots (the batch reader tries both locations per commit).
        let (_tmp, root) = repo();
        std::fs::create_dir_all(root.join(".void")).unwrap();
        std::fs::write(
            root.join(board::BOARD_FALLBACK),
            "## Backlog\n\n- **VB-1** Hidden board\n",
        )
        .unwrap();
        sh_git(&root, &["add", board::BOARD_FALLBACK]);
        sh_git(&root, &["commit", "-q", "-m", "v1"]);

        let hist = board_history(&root, "demo").unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].id, "VB-1");
        assert_eq!(hist[0].events[0].column, "Backlog");
        assert_ne!(hist[0].events[0].commit, UNCOMMITTED);
    }

    #[test]
    fn test_no_git_history_degrades_to_current_board() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("BOARD.md"),
            "## Backlog\n\n- **VB-1** Only\n",
        )
        .unwrap();
        let hist = board_history(tmp.path(), "demo").unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].current_column.as_deref(), Some("Backlog"));
        // The working-tree snapshot still opens the timeline.
        assert_eq!(hist[0].events.len(), 1);
        assert_eq!(hist[0].events[0].commit, UNCOMMITTED);
    }
}
