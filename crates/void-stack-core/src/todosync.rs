//! `void todo-sync`: mirror code markers (TODO/FIXME/HACK) into BOARD.md.
//!
//! Scans the project for `// TODO(name): ...`, `// FIXME: ...` and
//! `// HACK: ...` markers and syncs them as Backlog tasks with an
//! automatic link to the file and, when the structural graph is
//! available, the containing symbol. Markers are extracted ONLY from real
//! comments (tree-sitter comment nodes) in production code: string
//! literals never match, test files/modules are skipped (the audit's
//! module-role and test-scope detection), and the marker must follow the
//! strict `KEYWORD[(name)]:` form so prose like "TODO/FIXME/HACK markers"
//! in doc comments stays out. Idempotent: each marker gets a stable
//! content hash stored on the task (`sync:<hash>`), so re-runs never
//! duplicate. Markers that disappear from the code mark their task
//! resolved (moved to Done + `auto-resolved` tag) — never a silent
//! delete; `--clean` purges tasks whose marker no longer passes the
//! filter (garbage from earlier, laxer scans).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::analyzer::explicit_debt::ExplicitDebtItem;
use crate::board::{self, Board};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Marker kinds that become board tasks (the rest of the debt keywords
/// stay analysis-only noise). The tree-sitter path embeds them in
/// `marker_re`; the textual fallback filters on this list.
#[cfg(not(feature = "structural"))]
const SYNC_KINDS: [&str; 3] = ["TODO", "FIXME", "HACK"];
const MAX_TITLE: usize = 90;

const SKIP_DIRS: [&str; 16] = [
    "node_modules",
    ".git",
    "target",
    "build",
    "dist",
    ".dart_tool",
    "__pycache__",
    ".next",
    "vendor",
    ".venv",
    "venv",
    "coverage",
    // Unreal Engine / UEFN generated dirs (user code in Plugins/ is kept)
    "Intermediate",
    "Saved",
    "Binaries",
    "DerivedDataCache",
];

/// Strict marker form: keyword, optional `(assignee)`, mandatory colon.
/// Prose mentions ("TODO, FIXME", "TODO/FIXME/HACK markers") never match.
fn marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(TODO|FIXME|HACK)(\([^)]{1,30}\))?\s*:\s*(.*)$").unwrap())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TodoSyncReport {
    pub markers_found: usize,
    pub added: usize,
    pub unchanged: usize,
    pub resolved: usize,
    /// Tasks deleted by `--clean` (their marker no longer passes the
    /// comment-only filter).
    #[serde(default)]
    pub purged: usize,
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

/// Extract TODO/FIXME/HACK markers from REAL comments only: tree-sitter
/// comment nodes in production files. String literals never match; test
/// files/modules (audit module-role) and `#[cfg(test)]` scopes (audit
/// test-scope) are skipped.
#[cfg(feature = "structural")]
pub fn scan_markers(root: &Path) -> Vec<ExplicitDebtItem> {
    let ignore = crate::ignore::VoidIgnore::load(root);
    let mut items = Vec::new();
    walk_files(root, root, &ignore, &mut items, 0);
    items.sort_by(|a, b| (a.file.as_str(), a.line).cmp(&(b.file.as_str(), b.line)));
    items
}

/// Textual fallback when built without tree-sitter — still excludes
/// test-role files, but cannot see comment boundaries.
#[cfg(not(feature = "structural"))]
pub fn scan_markers(root: &Path) -> Vec<ExplicitDebtItem> {
    use crate::audit::findings::ModuleRole;
    crate::analyzer::explicit_debt::scan_explicit_debt(root)
        .into_iter()
        .filter(|i| SYNC_KINDS.contains(&i.kind.as_str()))
        .filter(|i| crate::audit::context::detect_module_role(&i.file) != ModuleRole::Test)
        .collect()
}

#[cfg(feature = "structural")]
fn walk_files(
    root: &Path,
    dir: &Path,
    ignore: &crate::ignore::VoidIgnore,
    items: &mut Vec<ExplicitDebtItem>,
    depth: u32,
) {
    use crate::audit::findings::ModuleRole;
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                continue;
            }
            walk_files(root, &path, ignore, items, depth + 1);
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if ignore.is_ignored(&rel) {
            continue;
        }
        // Test/example/generated modules never feed the board. The
        // leading slash lets root-level markers ("/tests/") match
        // project-relative paths.
        if matches!(
            crate::audit::context::detect_module_role(&format!("/{}", rel)),
            ModuleRole::Test | ModuleRole::Example | ModuleRole::Generated
        ) {
            continue;
        }
        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 1_048_576 {
            continue;
        }
        items.extend(file_comment_markers(&path, &rel));
    }
}

/// Parse one file and pull markers out of its comment nodes.
#[cfg(feature = "structural")]
fn file_comment_markers(abs: &Path, rel: &str) -> Vec<ExplicitDebtItem> {
    let Some(lang_name) = crate::structural::language_for(abs) else {
        return Vec::new();
    };
    let Some(language) = crate::structural::langs::load_language(lang_name) else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(abs) else {
        return Vec::new();
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(content.as_bytes(), None) else {
        return Vec::new();
    };

    let audit_lang = crate::audit::context::detect_language(rel);
    let mut items = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind().contains("comment") {
            let Ok(text) = node.utf8_text(content.as_bytes()) else {
                continue;
            };
            for (offset, raw_line) in text.lines().enumerate() {
                // Doc comments (///, //!) document APIs — they quote
                // marker syntax, they don't track work.
                let trimmed = raw_line.trim_start();
                if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                    continue;
                }
                let line = node.start_position().row + offset + 1;
                // `#[cfg(test)]` modules inside production files stay out.
                if crate::audit::context::in_test_scope(&content, line, audit_lang) {
                    continue;
                }
                let Some(caps) = marker_re().captures(raw_line) else {
                    continue;
                };
                let kind = caps[1].to_string();
                let assignee = caps.get(2).map(|m| m.as_str().to_string());
                let rest = caps[3].trim().trim_end_matches("*/").trim();
                // Reconstruct the after-keyword text so hashes stay
                // compatible with the previous scheme.
                let text = match &assignee {
                    Some(a) => format!("{}: {}", a, rest),
                    None => rest.to_string(),
                };
                items.push(ExplicitDebtItem {
                    file: rel.to_string(),
                    line,
                    kind,
                    text,
                    language: lang_name.to_string(),
                });
            }
            continue;
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
    items
}

/// Remove every synced task whose hash is not in `valid` — garbage from
/// earlier, laxer scans. Also catches corrupted rows whose `sync:` token
/// got mangled into the title (broken markdown from unsanitized titles).
/// All columns, Done included. Returns removed ids.
pub fn purge_stale_synced(board: &mut Board, valid: &HashSet<String>) -> Vec<String> {
    fn remnant_re() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"sync:[0-9a-f]{8}").unwrap())
    }
    let mut removed = Vec::new();
    for col in &mut board.columns {
        col.tasks.retain(|t| {
            let stale_hash = t.sync.as_ref().map(|h| !valid.contains(h)).unwrap_or(false);
            let corrupted = t.sync.is_none() && remnant_re().is_match(&t.title);
            if stale_hash || corrupted {
                removed.push(t.id.clone());
                false
            } else {
                true
            }
        });
    }
    removed
}

/// Scan the project and sync markers into the board. Loads and saves
/// BOARD.md itself; returns what changed.
pub fn sync_todos(project: &Project) -> Result<TodoSyncReport, String> {
    sync_todos_with(project, false)
}

/// Like [`sync_todos`], with `clean` purging stale synced tasks instead of
/// resolving them to Done.
pub fn sync_todos_with(project: &Project, clean: bool) -> Result<TodoSyncReport, String> {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    let mut board = board::load_board(&root, &project.name)?;
    let report = sync_into_board_opts(project, &root, &mut board, clean);
    if report.added > 0 || report.resolved > 0 || report.purged > 0 {
        board::save_board(&root, &board)?;
    }
    Ok(report)
}

/// Core sync on an in-memory board (unit-testable without saving).
pub fn sync_into_board(project: &Project, root: &Path, board: &mut Board) -> TodoSyncReport {
    sync_into_board_opts(project, root, board, false)
}

pub fn sync_into_board_opts(
    project: &Project,
    root: &Path,
    board: &mut Board,
    clean: bool,
) -> TodoSyncReport {
    let markers: Vec<ExplicitDebtItem> = scan_markers(root);

    // hash → marker (first occurrence wins on duplicates).
    let mut by_hash: HashMap<String, &ExplicitDebtItem> = HashMap::new();
    for m in &markers {
        by_hash.entry(marker_hash(m)).or_insert(m);
    }

    // `clean`: delete (don't resolve) tasks whose marker no longer passes
    // the filter — leftovers from earlier scans that saw string literals.
    let purged = if clean {
        let valid: HashSet<String> = by_hash.keys().cloned().collect();
        purge_stale_synced(board, &valid).len()
    } else {
        0
    };

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
        purged,
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
    fn test_string_literals_never_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.rs"),
            "fn f() {\n    let s = \"// TODO: fake marker in a literal\";\n    let t = \"content with FIXME: inside\\n\";\n}\n",
        )
        .unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);
        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.markers_found, 0, "{:?}", report);
        assert_eq!(report.added, 0);
    }

    #[test]
    fn test_test_files_and_cfg_test_modules_skipped() {
        let dir = tempfile::tempdir().unwrap();
        // A test-role file: everything ignored.
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(
            dir.path().join("tests/it.rs"),
            "// TODO: only exists in a test file\nfn t() {}\n",
        )
        .unwrap();
        // A production file whose cfg(test) module has a marker.
        std::fs::write(
            dir.path().join("lib.rs"),
            "// TODO(edu): real production marker\npub fn f() {}\n\n#[cfg(test)]\nmod tests {\n    // TODO: marker inside the test module\n    fn t() {}\n}\n",
        )
        .unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);
        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.added, 1, "{:?}", report);
        assert_eq!(board.columns[0].tasks[0].title, "real production marker");
    }

    #[test]
    fn test_prose_mentions_without_colon_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.rs"),
            "//! Sync TODO/FIXME/HACK code markers into the board.\n// Scans for: TODO, FIXME, HACK keywords.\n// TODO: this one is real\nfn f() {}\n",
        )
        .unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);
        let report = sync_into_board(&project, dir.path(), &mut board);
        assert_eq!(report.added, 1, "{:?}", report);
        assert_eq!(board.columns[0].tasks[0].title, "this one is real");
    }

    #[test]
    fn test_clean_purges_stale_synced_tasks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.rs"),
            "// TODO: legit marker\nfn f() {}\n",
        )
        .unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);
        // Garbage from an earlier, laxer scan: synced hash that no current
        // marker produces — spread across columns.
        board::add_task(&mut board, "garbage literal", None, &[], "2026-07-01");
        board.columns[0].tasks[0].sync = Some("deadbeef".into());
        board::add_task(&mut board, "manual task", None, &[], "2026-07-01");
        let report = sync_into_board_opts(&project, dir.path(), &mut board, true);
        assert_eq!(report.purged, 1, "{:?}", report);
        assert_eq!(report.added, 1);
        assert!(
            board.find_task("VB-1").is_none(),
            "garbage deleted, not resolved"
        );
        assert!(
            board
                .columns
                .iter()
                .flat_map(|c| &c.tasks)
                .any(|t| t.title == "manual task"),
            "manual tasks always survive --clean"
        );
    }

    #[test]
    fn test_clean_purges_corrupted_remnant_rows() {
        // A row whose sync token got mangled into the title by broken
        // markdown: no sync hash, but the remnant is visible.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.rs"),
            "fn f() {}
",
        )
        .unwrap();
        let project = fixture(dir.path());
        let mut board = Board::new(&project.name);
        board::add_task(
            &mut board,
            "...// FIXME: ...#todo#name2026-07-09sync:e440fb62",
            None,
            &[],
            "2026-07-09",
        );
        board::add_task(&mut board, "manual task", None, &[], "2026-07-09");
        let report = sync_into_board_opts(&project, dir.path(), &mut board, true);
        assert_eq!(report.purged, 1, "{:?}", report);
        let titles: Vec<&str> = board
            .columns
            .iter()
            .flat_map(|c| &c.tasks)
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(titles, vec!["manual task"]);
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
