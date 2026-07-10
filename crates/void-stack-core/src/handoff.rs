//! `void handoff`: session journal for context transfer between sessions
//! (or machines — the journal lives in the repo and is committable).
//!
//! Captures what a work session leaves behind: today's commits, the
//! uncommitted diff with its touched symbols, which board tasks moved or
//! are in flight, and what's half-done (uncommitted hunks, changed symbols
//! with no covering tests). Saved to `.void/journal/YYYY-MM-DD-HHmm.md`
//! plus a `LATEST.md` copy that `session_context` reads on the next
//! session start.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

pub const JOURNAL_DIR: &str = ".void/journal";
pub const LATEST_FILE: &str = "LATEST.md";
const MAX_COMMITS: usize = 15;
const MAX_FILES: usize = 20;
const MAX_SYMBOLS: usize = 20;

/// Build the handoff markdown for the current session state.
pub fn generate_handoff(project: &Project, note: Option<&str>) -> Result<String, String> {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    let now = chrono::Local::now();

    let mut md = format!(
        "# Handoff — {} ({})\n",
        project.name,
        now.format("%Y-%m-%d %H:%M")
    );
    if let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) {
        md.push_str(&format!("\n> {}\n", note));
    }

    md.push_str(&commits_section(&root));
    md.push_str(&diff_sections(project, &root));
    md.push_str(&board_section(&root, &project.name));
    Ok(md)
}

/// Today's commits (local midnight cutoff).
fn commits_section(root: &Path) -> String {
    let out = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "log",
            "--oneline",
            "--since=00:00",
        ])
        .output();
    let mut md = String::from("\n## Commits today\n");
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let lines: Vec<&str> = text.lines().collect();
            if lines.is_empty() {
                md.push_str("- none\n");
            }
            for l in lines.iter().take(MAX_COMMITS) {
                md.push_str(&format!("- {}\n", l));
            }
            if lines.len() > MAX_COMMITS {
                md.push_str(&format!("- (+{} more)\n", lines.len() - MAX_COMMITS));
            }
        }
        _ => md.push_str("- n/a (not a git repository?)\n"),
    }
    md
}

/// Uncommitted work: files + touched symbols + coverage gaps.
fn diff_sections(project: &Project, root: &Path) -> String {
    let hunks = crate::diff::get_changed_hunks(root, None);
    if hunks.is_empty() {
        return "\n## Uncommitted work\n- clean working tree — everything is committed\n"
            .to_string();
    }
    let mut md = format!("\n## Uncommitted work — {} file(s)\n", hunks.len());
    for h in hunks.iter().take(MAX_FILES) {
        md.push_str(&format!(
            "- {:?} `{}` (+{} / -{})\n",
            h.status, h.file, h.added, h.removed
        ));
    }
    if hunks.len() > MAX_FILES {
        md.push_str(&format!("- (+{} more files)\n", hunks.len() - MAX_FILES));
    }

    #[cfg(feature = "structural")]
    {
        if let Ok(conn) = crate::structural::open_db(project) {
            let symbols = crate::diff::hunks_to_symbols(&conn, &hunks);
            let named: Vec<_> = symbols.iter().filter(|s| s.kind != "file").collect();
            if !named.is_empty() {
                md.push_str(&format!("\n### Symbols touched ({})\n", named.len()));
                for s in named.iter().take(MAX_SYMBOLS) {
                    md.push_str(&format!(
                        "- {} `{}` — {}:{}\n",
                        s.kind, s.name, s.file, s.line_start
                    ));
                }
                if named.len() > MAX_SYMBOLS {
                    md.push_str(&format!("- (+{} more)\n", named.len() - MAX_SYMBOLS));
                }
            }

            // Half-done signal: changed symbols with zero covering tests.
            if crate::testing::ensure_coverage_map(&conn, crate::testing::DEFAULT_COVERAGE_DEPTH)
                .is_ok()
                && let Ok(suggestions) = crate::testing::suggest_for_symbols(&conn, &symbols, 10)
                && !suggestions.uncovered.is_empty()
            {
                md.push_str(&format!(
                    "\n### Uncovered ({}) — changed symbols with no covering tests\n",
                    suggestions.uncovered.len()
                ));
                for u in suggestions.uncovered.iter().take(MAX_SYMBOLS) {
                    md.push_str(&format!("- `{}` — {}:{}\n", u.name, u.file, u.line_start));
                }
            }
        }
    }
    #[cfg(not(feature = "structural"))]
    let _ = project;

    md
}

/// Board snapshot: in-flight tasks + tasks this diff touches.
fn board_section(root: &Path, project_name: &str) -> String {
    let Ok(board) = crate::board::load_board(root, project_name) else {
        return String::new();
    };
    let mut md = String::new();
    for col in &board.columns {
        let in_flight =
            col.name.eq_ignore_ascii_case("Doing") || col.name.eq_ignore_ascii_case("Review");
        if !in_flight || col.tasks.is_empty() {
            continue;
        }
        md.push_str(&format!(
            "\n## Board — {} ({})\n",
            col.name,
            col.tasks.len()
        ));
        for t in &col.tasks {
            md.push_str(&format!("- **{}** {}", t.id, t.title));
            if !t.links.is_empty() {
                md.push_str(&format!(" → {}", t.links.join(", ")));
            }
            md.push('\n');
        }
    }
    let files: Vec<String> = crate::diff::get_changed_hunks(root, None)
        .iter()
        .map(|h| h.file.clone())
        .collect();
    if !files.is_empty() {
        let hits = crate::board::tasks_touching(&board, &files, &[]);
        if !hits.is_empty() {
            md.push_str("\n## Tasks this diff touches\n");
            for (col, t) in hits {
                md.push_str(&format!("- **{}** {} ({})\n", t.id, t.title, col));
            }
        }
    }
    md
}

/// Persist to `.void/journal/YYYY-MM-DD-HHmm.md` and refresh `LATEST.md`
/// (a plain copy — symlinks don't survive all filesystems/git configs).
pub fn save_handoff(
    project_root: &Path,
    markdown: &str,
    now: chrono::DateTime<chrono::Local>,
) -> Result<PathBuf, String> {
    let dir = project_root.join(JOURNAL_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    let path = dir.join(format!("{}.md", now.format("%Y-%m-%d-%H%M")));
    std::fs::write(&path, markdown)
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
    std::fs::write(dir.join(LATEST_FILE), markdown)
        .map_err(|e| format!("cannot write {}: {}", dir.join(LATEST_FILE).display(), e))?;
    Ok(path)
}

/// The most recent handoff, if any (what session_context surfaces).
pub fn latest_handoff(project_root: &Path) -> Option<String> {
    let path = project_root.join(JOURNAL_DIR).join(LATEST_FILE);
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success(), "git {:?}: {:?}", args, st);
    }

    fn fixture(dir: &Path) -> Project {
        Project {
            name: "handoff-demo".into(),
            path: dir.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    #[test]
    fn test_handoff_captures_session_state() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(
            dir.path().join("BOARD.md"),
            "## Doing\n\n- **VB-7** In flight\n  - link: a.rs\n",
        )
        .unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "feat: session work"]);
        // Leave something half-done.
        std::fs::write(dir.path().join("a.rs"), "fn a() { let _x = 1; }\n").unwrap();

        let project = fixture(dir.path());
        let md = generate_handoff(&project, Some("stopping for lunch")).unwrap();

        assert!(md.contains("# Handoff — handoff-demo"));
        assert!(md.contains("> stopping for lunch"));
        assert!(md.contains("feat: session work"), "{md}");
        assert!(md.contains("Uncommitted work — 1 file(s)"), "{md}");
        assert!(md.contains("## Board — Doing (1)"));
        assert!(md.contains("## Tasks this diff touches"), "{md}");
        assert!(md.contains("VB-7"));
    }

    #[test]
    fn test_handoff_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "base"]);

        let md = generate_handoff(&fixture(dir.path()), None).unwrap();
        assert!(md.contains("clean working tree"));
        assert!(!md.contains("Tasks this diff touches"));
    }

    #[test]
    fn test_save_and_latest_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::Local::now();
        let path = save_handoff(dir.path(), "# Handoff one\n", now).unwrap();
        assert!(path.to_string_lossy().contains(".void/journal/"));
        assert_eq!(latest_handoff(dir.path()).unwrap(), "# Handoff one\n");
        // A second save refreshes LATEST.
        save_handoff(dir.path(), "# Handoff two\n", now).unwrap();
        assert_eq!(latest_handoff(dir.path()).unwrap(), "# Handoff two\n");
    }
}
