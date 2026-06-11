//! Shared git-diff plumbing: changed hunks and their mapping to structural
//! symbols. Used by `suggest_tests_for_diff` and `review_diff`.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::process_util::HideWindow;

// ── Model ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// Changed line ranges for one file, parsed from `git diff --unified=0`.
#[derive(Debug, Clone, Serialize)]
pub struct FileHunks {
    /// Path in the NEW tree (rename target). POSIX separators.
    pub file: String,
    /// Original path for renames.
    pub old_file: Option<String>,
    pub status: ChangeStatus,
    /// Changed line ranges in the new file, inclusive. For pure deletions
    /// the surrounding position is recorded as a 1-line touch point so the
    /// enclosing symbol still counts as changed. Empty for deleted files.
    pub ranges: Vec<(usize, usize)>,
    pub added: usize,
    pub removed: usize,
}

/// A structural symbol overlapped by a changed hunk.
#[derive(Debug, Clone, Serialize)]
pub struct ChangedSymbol {
    pub qualified_name: String,
    pub name: String,
    /// Node kind (`Function`, `Class`, `Test`, ...) or `file` for hunks
    /// outside any node / files not in the graph.
    pub kind: String,
    pub language: String,
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub is_test: bool,
    /// True when the file has no nodes in the structural graph yet
    /// (new file, or graph built before it existed).
    pub is_new_file: bool,
}

// ── Hunk extraction ─────────────────────────────────────────

/// Parse `git diff [base] --unified=0` into per-file changed line ranges.
///
/// With `git_base = None` the diff is taken against `HEAD` (working tree +
/// staged). Renames are followed (`-M`), new/deleted files handled, and
/// non-UTF8 output is read lossily so a binary-ish hunk never aborts the
/// whole parse.
pub fn get_changed_hunks(project_path: &Path, git_base: Option<&str>) -> Vec<FileHunks> {
    if !project_path.join(".git").exists() {
        return Vec::new();
    }
    let base = git_base.unwrap_or("HEAD");
    let project_arg = project_path.to_string_lossy().to_string();

    let output = Command::new("git")
        .args([
            "-C",
            &project_arg,
            "diff",
            "-M",
            "--no-color",
            "--unified=0",
            base,
            "--",
        ])
        .hide_window()
        .output();

    let Ok(out) = output else {
        return Vec::new();
    };
    if !out.status.success() {
        // e.g. unborn HEAD or unknown ref — nothing to diff against.
        return Vec::new();
    }
    parse_unified_zero(&String::from_utf8_lossy(&out.stdout))
}

/// Parse the `--unified=0` diff text. Exposed for unit tests.
pub(crate) fn parse_unified_zero(diff: &str) -> Vec<FileHunks> {
    let mut out: Vec<FileHunks> = Vec::new();
    let mut current: Option<FileHunks> = None;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(f) = current.take() {
                out.push(f);
            }
            current = Some(FileHunks {
                file: String::new(),
                old_file: None,
                status: ChangeStatus::Modified,
                ranges: Vec::new(),
                added: 0,
                removed: 0,
            });
            continue;
        }
        let Some(f) = current.as_mut() else { continue };

        if let Some(rest) = line.strip_prefix("rename from ") {
            f.old_file = Some(rest.trim().to_string());
            f.status = ChangeStatus::Renamed;
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            f.file = rest.trim().to_string();
        } else if line.starts_with("new file mode") {
            f.status = ChangeStatus::Added;
        } else if line.starts_with("deleted file mode") {
            f.status = ChangeStatus::Deleted;
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            let p = rest.trim();
            if p != "/dev/null" {
                f.file = p.strip_prefix("b/").unwrap_or(p).to_string();
            }
        } else if let Some(rest) = line.strip_prefix("--- ") {
            let p = rest.trim();
            if p != "/dev/null" && f.file.is_empty() && f.old_file.is_none() {
                // Keep the old path around for deleted files so the entry
                // still names the file.
                f.old_file = Some(p.strip_prefix("a/").unwrap_or(p).to_string());
            }
        } else if let Some(range) = parse_hunk_header(line) {
            f.ranges.push(range);
        } else if line.starts_with('+') && !line.starts_with("+++") {
            f.added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            f.removed += 1;
        }
    }
    if let Some(f) = current.take() {
        out.push(f);
    }

    // Deleted files: name them via the old path, no new-file ranges.
    for f in &mut out {
        if f.file.is_empty()
            && let Some(old) = &f.old_file
        {
            f.file = old.clone();
            f.ranges.clear();
        }
        f.file = f.file.replace('\\', "/");
    }
    out.retain(|f| !f.file.is_empty());
    out
}

/// `@@ -a,b +c,d @@` → changed range in the new file. `d` omitted means 1;
/// `d = 0` is a pure deletion at position `c` — recorded as a 1-line touch
/// point so the enclosing symbol still registers as changed.
fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ ")?;
    let plus = rest.split(' ').find(|p| p.starts_with('+'))?;
    let nums = plus.trim_start_matches('+');
    let (start, count) = match nums.split_once(',') {
        Some((s, c)) => (s.parse::<usize>().ok()?, c.parse::<usize>().ok()?),
        None => (nums.parse::<usize>().ok()?, 1),
    };
    if count == 0 {
        let touch = start.max(1);
        return Some((touch, touch));
    }
    Some((start, start + count - 1))
}

// ── Symbol mapping ──────────────────────────────────────────

/// Map changed hunks to structural nodes by (file, line-range overlap).
/// Hunks outside any node produce a file-level entry; files absent from
/// the graph are marked `is_new_file`.
#[cfg(feature = "structural")]
pub fn hunks_to_symbols(conn: &rusqlite::Connection, hunks: &[FileHunks]) -> Vec<ChangedSymbol> {
    let mut out: Vec<ChangedSymbol> = Vec::new();

    for fh in hunks {
        if fh.status == ChangeStatus::Deleted {
            continue;
        }
        let nodes = nodes_for_file(conn, &fh.file);
        if nodes.is_empty() {
            out.push(file_level_symbol(fh, true));
            continue;
        }

        let mut any_overlap = false;
        let mut covered_all_ranges = true;
        for &(start, end) in &fh.ranges {
            let mut range_covered = false;
            for n in &nodes {
                // Skip the file-level node itself (line range 0/whole file).
                if !n.qualified_name.contains("::") {
                    continue;
                }
                if n.line_start <= end && n.line_end >= start {
                    range_covered = true;
                    any_overlap = true;
                    if !out
                        .iter()
                        .any(|s: &ChangedSymbol| s.qualified_name == n.qualified_name)
                    {
                        out.push(ChangedSymbol {
                            qualified_name: n.qualified_name.clone(),
                            name: n.name.clone(),
                            kind: n.kind.as_str().to_string(),
                            language: n.language.clone(),
                            file: fh.file.clone(),
                            line_start: n.line_start,
                            line_end: n.line_end,
                            is_test: n.is_test
                                || matches!(n.kind, crate::structural::NodeKind::Test),
                            is_new_file: false,
                        });
                    }
                }
            }
            if !range_covered {
                covered_all_ranges = false;
            }
        }

        // Hunks outside every node (imports, consts, module docs).
        if !any_overlap || !covered_all_ranges {
            let key = format!("file:{}", fh.file);
            if !out.iter().any(|s| s.qualified_name == key) {
                out.push(file_level_symbol(fh, false));
            }
        }
    }

    out
}

#[cfg(feature = "structural")]
fn file_level_symbol(fh: &FileHunks, is_new_file: bool) -> ChangedSymbol {
    ChangedSymbol {
        qualified_name: format!("file:{}", fh.file),
        name: fh.file.rsplit('/').next().unwrap_or(&fh.file).to_string(),
        kind: "file".to_string(),
        language: String::new(),
        file: fh.file.clone(),
        line_start: fh.ranges.first().map(|r| r.0).unwrap_or(1),
        line_end: fh.ranges.last().map(|r| r.1).unwrap_or(1),
        is_test: false,
        is_new_file,
    }
}

#[cfg(feature = "structural")]
fn nodes_for_file(
    conn: &rusqlite::Connection,
    file: &str,
) -> Vec<crate::structural::StructuralNode> {
    let backslash = file.replace('/', "\\");
    let mut stmt = match conn.prepare(
        "SELECT kind, name, qualified_name, file_path, line_start, line_end, \
         language, parent_name, is_test FROM nodes \
         WHERE file_path = ?1 OR file_path = ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(rusqlite::params![file, backslash], |row| {
        let kind_str: String = row.get(0)?;
        Ok(crate::structural::StructuralNode {
            kind: crate::structural::NodeKind::parse(&kind_str)
                .unwrap_or(crate::structural::NodeKind::Function),
            name: row.get(1)?,
            qualified_name: row.get(2)?,
            file_path: row.get(3)?,
            line_start: row.get::<_, i64>(4)? as usize,
            line_end: row.get::<_, i64>(5)? as usize,
            language: row.get(6)?,
            parent_name: row.get(7)?,
            is_test: row.get::<_, i64>(8)? != 0,
        })
    });
    match rows {
        Ok(r) => r.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

// ── Tests ───────────────────────────────────────────────────

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

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "t@t"]);
        git(dir, &["config", "user.name", "t"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
    }

    #[test]
    fn test_changed_hunks_modify_add_delete_rename() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
        std::fs::write(dir.path().join("gone.rs"), "fn gone() {}\n").unwrap();
        std::fs::write(dir.path().join("moveme.rs"), "fn moved() {}\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "base"]);

        // Modify line 2, add a file, delete one, rename one.
        std::fs::write(
            dir.path().join("a.rs"),
            "fn a() {}\nfn b() { changed(); }\nfn c() {}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("new.rs"), "fn brand_new() {}\n").unwrap();
        std::fs::remove_file(dir.path().join("gone.rs")).unwrap();
        std::fs::rename(dir.path().join("moveme.rs"), dir.path().join("renamed.rs")).unwrap();
        git(dir.path(), &["add", "-A"]);

        let hunks = get_changed_hunks(dir.path(), None);
        let by_file = |f: &str| hunks.iter().find(|h| h.file == f);

        let a = by_file("a.rs").expect("a.rs hunk");
        assert_eq!(a.status, ChangeStatus::Modified);
        assert_eq!(a.ranges, vec![(2, 2)]);
        assert_eq!(a.added, 1);
        assert_eq!(a.removed, 1);

        let new = by_file("new.rs").expect("new.rs hunk");
        assert_eq!(new.status, ChangeStatus::Added);
        assert_eq!(new.ranges, vec![(1, 1)]);

        let gone = by_file("gone.rs").expect("deleted file entry");
        assert_eq!(gone.status, ChangeStatus::Deleted);
        assert!(gone.ranges.is_empty());

        let renamed = by_file("renamed.rs").expect("rename entry");
        assert_eq!(renamed.status, ChangeStatus::Renamed);
        assert_eq!(renamed.old_file.as_deref(), Some("moveme.rs"));
    }

    #[test]
    fn test_changed_hunks_no_repo_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(get_changed_hunks(dir.path(), None).is_empty());
    }

    #[test]
    fn test_parse_hunk_header_forms() {
        assert_eq!(parse_hunk_header("@@ -1,2 +3,4 @@"), Some((3, 6)));
        assert_eq!(parse_hunk_header("@@ -5 +7 @@ fn x()"), Some((7, 7)));
        // Pure deletion: d = 0 → 1-line touch point.
        assert_eq!(parse_hunk_header("@@ -9,3 +8,0 @@"), Some((8, 8)));
        assert_eq!(parse_hunk_header("not a hunk"), None);
    }

    #[cfg(feature = "structural")]
    #[test]
    fn test_hunks_to_symbols_overlap_and_file_level() {
        let dir = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: format!("diff-fixture-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let conn = crate::structural::open_db(&project).unwrap();
        let node =
            |name: &str, ls: usize, le: usize, test: bool| crate::structural::StructuralNode {
                kind: if test {
                    crate::structural::NodeKind::Test
                } else {
                    crate::structural::NodeKind::Function
                },
                name: name.to_string(),
                qualified_name: format!("src/a.rs::{}", name),
                file_path: "src/a.rs".to_string(),
                line_start: ls,
                line_end: le,
                language: "rust".to_string(),
                parent_name: None,
                is_test: test,
            };
        crate::structural::store_file(
            &conn,
            "src/a.rs",
            &[node("alpha", 10, 20, false), node("beta", 30, 40, false)],
            &[],
            "h",
        )
        .unwrap();

        let hunks = vec![
            FileHunks {
                file: "src/a.rs".into(),
                old_file: None,
                status: ChangeStatus::Modified,
                // Overlaps alpha; line 1 is outside every node.
                ranges: vec![(12, 14), (1, 1)],
                added: 3,
                removed: 0,
            },
            FileHunks {
                file: "src/unknown.rs".into(),
                old_file: None,
                status: ChangeStatus::Added,
                ranges: vec![(1, 5)],
                added: 5,
                removed: 0,
            },
        ];
        let syms = hunks_to_symbols(&conn, &hunks);

        assert!(
            syms.iter()
                .any(|s| s.qualified_name == "src/a.rs::alpha" && !s.is_new_file),
            "alpha must be detected: {:?}",
            syms.iter().map(|s| &s.qualified_name).collect::<Vec<_>>()
        );
        assert!(
            !syms.iter().any(|s| s.qualified_name == "src/a.rs::beta"),
            "beta untouched"
        );
        assert!(
            syms.iter()
                .any(|s| s.qualified_name == "file:src/a.rs" && s.kind == "file"),
            "out-of-node hunk must produce a file-level entry"
        );
        assert!(
            syms.iter()
                .any(|s| s.file == "src/unknown.rs" && s.is_new_file),
            "file missing from graph must be marked new"
        );
    }
}
