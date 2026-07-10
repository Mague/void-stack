//! `void commit`: conventional commit messages inferred from the diff.
//!
//! Builds a `type(scope): subject` message from the current working-tree
//! diff: the type comes from simple diff-shape heuristics (docs-only,
//! tests-only, new files, deletion-heavy...), the scope from the dominant
//! area of the diff (weighted by symbols touched per file when the
//! structural graph is available), and the body references the open board
//! tasks the diff touches (Fase 1 detection). Committing moves those
//! tasks to Done first so BOARD.md rides along in the same commit.
//! The MCP surface (`suggest_commit_message`) only suggests — the CLI is
//! the only thing that ever runs `git commit`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::board;
use crate::diff::{ChangeStatus, FileHunks, get_changed_hunks};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

const MAX_SUBJECT: usize = 72;

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommitSuggestion {
    pub commit_type: String,
    pub scope: Option<String>,
    pub subject: String,
    /// Full message: subject line + blank line + body.
    pub message: String,
    /// Board task ids this diff appears to resolve (moved to Done on
    /// actual commit, referenced in the body either way).
    pub resolves: Vec<String>,
}

/// Build a conventional-commit suggestion for the current diff vs HEAD.
pub fn suggest_commit_message(project: &Project) -> Result<CommitSuggestion, String> {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    let hunks = get_changed_hunks(&root, None);
    if hunks.is_empty() {
        return Err("clean working tree — nothing to commit".to_string());
    }

    // Symbols per file (weights the scope + names the subject); empty
    // without a structural graph.
    let mut symbols_by_file: HashMap<String, Vec<String>> = HashMap::new();
    #[cfg(feature = "structural")]
    if let Ok(conn) = crate::structural::open_db(project) {
        for s in crate::diff::hunks_to_symbols(&conn, &hunks) {
            if s.kind != "file" {
                symbols_by_file
                    .entry(s.file.clone())
                    .or_default()
                    .push(s.name);
            }
        }
    }

    let commit_type = infer_type(&hunks, &symbols_by_file);
    let scope = infer_scope(&hunks, &symbols_by_file);

    // Board tasks the diff touches.
    let files: Vec<String> = hunks.iter().map(|h| h.file.clone()).collect();
    let symbol_names: Vec<String> = symbols_by_file.values().flatten().cloned().collect();
    let b = board::load_board(&root, &project.name)?;
    let touched = board::tasks_touching(&b, &files, &symbol_names);
    let resolves: Vec<String> = touched.iter().map(|(_, t)| t.id.clone()).collect();

    // Subject: one resolved task's title beats a synthetic summary.
    let mut subject = if touched.len() == 1 {
        lowercase_first(&touched[0].1.title)
    } else {
        synth_subject(&hunks, &symbols_by_file)
    };
    if subject.len() > MAX_SUBJECT {
        let mut cut = MAX_SUBJECT;
        while !subject.is_char_boundary(cut) {
            cut -= 1;
        }
        subject.truncate(cut);
        subject.push('…');
    }

    let header = match &scope {
        Some(s) => format!("{}({}): {}", commit_type, s, subject),
        None => format!("{}: {}", commit_type, subject),
    };

    let added: usize = hunks.iter().map(|h| h.added).sum();
    let removed: usize = hunks.iter().map(|h| h.removed).sum();
    let mut body = format!(
        "{} file(s) changed, +{} / -{}.",
        hunks.len(),
        added,
        removed
    );
    for (col, task) in &touched {
        body.push_str(&format!(
            "\n\nResolves {} — {} (was {}).",
            task.id, task.title, col
        ));
    }

    Ok(CommitSuggestion {
        commit_type,
        scope,
        subject,
        message: format!("{}\n\n{}\n", header, body),
        resolves,
    })
}

/// Run the real commit: move resolved tasks to Done (so BOARD.md rides in
/// the same commit), stage tracked changes and `git commit`. Returns the
/// new commit's short line.
pub fn perform_commit(project: &Project, suggestion: &CommitSuggestion) -> Result<String, String> {
    let root = PathBuf::from(strip_win_prefix(&project.path));

    if !suggestion.resolves.is_empty() {
        let mut b = board::load_board(&root, &project.name)?;
        for id in &suggestion.resolves {
            let _ = board::move_task(&mut b, id, "Done");
        }
        board::save_board(&root, &b)?;
        // The board file may be untracked on its first commit.
        let board_file = board::board_path(&root);
        let _ = git(&root, &["add", &board_file.to_string_lossy()]);
    }

    // Stage modifications/deletions of tracked files — exactly the set the
    // message was computed from (untracked files stay out on purpose).
    git(&root, &["add", "-u"])?;
    git(&root, &["commit", "-m", &suggestion.message])?;
    git(&root, &["log", "--oneline", "-1"])
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(["-C", &root.to_string_lossy()])
        .args(args)
        .output()
        .map_err(|e| format!("git {:?}: {}", args, e))?;
    if !out.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn is_doc(file: &str) -> bool {
    let f = file.to_ascii_lowercase();
    f.ends_with(".md") || f.ends_with(".txt") || f.starts_with("docs/")
}

fn is_config(file: &str) -> bool {
    let f = file.to_ascii_lowercase();
    f.ends_with(".toml")
        || f.ends_with(".json")
        || f.ends_with(".yaml")
        || f.ends_with(".yml")
        || f.ends_with(".lock")
        || f.ends_with(".gitignore")
}

fn is_test_file(file: &str) -> bool {
    let f = file.to_ascii_lowercase();
    f.contains("/tests/")
        || f.contains("_test.")
        || f.contains(".test.")
        || f.contains(".spec.")
        || f.starts_with("tests/")
}

/// Diff-shape heuristics, most specific first.
fn infer_type(hunks: &[FileHunks], _symbols: &HashMap<String, Vec<String>>) -> String {
    if hunks.iter().all(|h| is_doc(&h.file)) {
        return "docs".into();
    }
    if hunks.iter().all(|h| is_test_file(&h.file)) {
        return "test".into();
    }
    if hunks.iter().all(|h| is_config(&h.file) || is_doc(&h.file)) {
        return "chore".into();
    }
    // New source files → a feature is being added.
    if hunks
        .iter()
        .any(|h| h.status == ChangeStatus::Added && !is_test_file(&h.file) && !is_doc(&h.file))
    {
        return "feat".into();
    }
    let added: usize = hunks.iter().map(|h| h.added).sum();
    let removed: usize = hunks.iter().map(|h| h.removed).sum();
    // Deletion-heavy or file-moving diffs read as refactors.
    if removed > added || hunks.iter().any(|h| h.status == ChangeStatus::Renamed) {
        return "refactor".into();
    }
    "fix".into()
}

/// Dominant area of the diff. Files are weighted by symbols touched when
/// the graph resolved any; path components like crates/src/packages are
/// skipped so `crates/void-stack-core/src/board.rs` scopes to
/// `void-stack-core`.
fn infer_scope(hunks: &[FileHunks], symbols: &HashMap<String, Vec<String>>) -> Option<String> {
    const GENERIC: [&str; 8] = [
        "crates", "src", "packages", "apps", "lib", "libs", "app", ".",
    ];
    let mut weight: HashMap<String, usize> = HashMap::new();
    for h in hunks {
        let w = symbols.get(&h.file).map(|s| s.len()).unwrap_or(0) + 1;
        let mut components = h.file.split('/');
        let mut area = components.next()?.to_string();
        if GENERIC.contains(&area.as_str())
            && let Some(second) = components.next()
        {
            area = second.to_string();
        }
        // A bare filename at the repo root scopes to its stem.
        if area.contains('.') {
            area = area.split('.').next().unwrap_or(&area).to_string();
        }
        *weight.entry(area).or_default() += w;
    }
    let total: usize = weight.values().sum();
    let (best, w) = weight.into_iter().max_by_key(|(_, w)| *w)?;
    // Only claim a scope when one area clearly dominates.
    if total > 0 && w * 2 >= total && !best.is_empty() {
        Some(best)
    } else {
        None
    }
}

fn synth_subject(hunks: &[FileHunks], symbols: &HashMap<String, Vec<String>>) -> String {
    let mut names: Vec<&String> = symbols.values().flatten().collect();
    names.sort();
    names.dedup();
    if !names.is_empty() {
        let shown: Vec<&str> = names.iter().take(3).map(|s| s.as_str()).collect();
        let extra = names.len().saturating_sub(3);
        let mut s = format!("update {}", shown.join(", "));
        if extra > 0 {
            s.push_str(&format!(" (+{} more)", extra));
        }
        return s;
    }
    if hunks.len() == 1 {
        let stem = hunks[0]
            .file
            .rsplit('/')
            .next()
            .unwrap_or(&hunks[0].file)
            .to_string();
        return format!("update {}", stem);
    }
    format!("update {} files", hunks.len())
}

fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_t(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success(), "git {:?}: {:?}", args, st);
    }

    fn fixture(dir: &Path) -> Project {
        Project {
            name: "commit-demo".into(),
            path: dir.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    fn hunk(file: &str, status: ChangeStatus, added: usize, removed: usize) -> FileHunks {
        FileHunks {
            file: file.to_string(),
            old_file: None,
            status,
            ranges: vec![],
            added,
            removed,
        }
    }

    #[test]
    fn test_infer_type_shapes() {
        let none = HashMap::new();
        let docs = vec![hunk("README.md", ChangeStatus::Modified, 5, 1)];
        assert_eq!(infer_type(&docs, &none), "docs");

        let tests = vec![hunk("tests/it.rs", ChangeStatus::Modified, 5, 1)];
        assert_eq!(infer_type(&tests, &none), "test");

        let chore = vec![
            hunk("Cargo.toml", ChangeStatus::Modified, 1, 1),
            hunk("CHANGELOG.md", ChangeStatus::Modified, 3, 0),
        ];
        assert_eq!(infer_type(&chore, &none), "chore");

        let feat = vec![
            hunk("crates/core/src/new_mod.rs", ChangeStatus::Added, 100, 0),
            hunk("crates/core/src/lib.rs", ChangeStatus::Modified, 1, 0),
        ];
        assert_eq!(infer_type(&feat, &none), "feat");

        let refactor = vec![hunk(
            "crates/core/src/big.rs",
            ChangeStatus::Modified,
            10,
            60,
        )];
        assert_eq!(infer_type(&refactor, &none), "refactor");

        let fix = vec![hunk("crates/core/src/big.rs", ChangeStatus::Modified, 8, 2)];
        assert_eq!(infer_type(&fix, &none), "fix");
    }

    #[test]
    fn test_infer_scope_dominant_area() {
        let none = HashMap::new();
        let hunks = vec![
            hunk(
                "crates/void-stack-core/src/board.rs",
                ChangeStatus::Modified,
                5,
                1,
            ),
            hunk(
                "crates/void-stack-core/src/lib.rs",
                ChangeStatus::Modified,
                1,
                0,
            ),
            hunk(
                "crates/void-stack-cli/src/main.rs",
                ChangeStatus::Modified,
                2,
                0,
            ),
        ];
        assert_eq!(
            infer_scope(&hunks, &none).as_deref(),
            Some("void-stack-core")
        );

        // No dominant area → no scope.
        let spread = vec![
            hunk("crates/a/src/x.rs", ChangeStatus::Modified, 1, 0),
            hunk("crates/b/src/y.rs", ChangeStatus::Modified, 1, 0),
            hunk("crates/c/src/z.rs", ChangeStatus::Modified, 1, 0),
        ];
        assert_eq!(infer_scope(&spread, &none), None);

        // Symbol weight beats file count.
        let mut sym = HashMap::new();
        sym.insert(
            "crates/b/src/y.rs".to_string(),
            vec!["a".into(), "b".into(), "c".into(), "d".into()],
        );
        assert_eq!(infer_scope(&spread, &sym).as_deref(), Some("b"));
    }

    #[test]
    fn test_suggest_and_commit_resolves_board_task() {
        let dir = tempfile::tempdir().unwrap();
        git_t(dir.path(), &["init", "-q"]);
        git_t(dir.path(), &["config", "user.email", "t@t"]);
        git_t(dir.path(), &["config", "user.name", "t"]);
        git_t(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("auth.py"), "def login():\n    return 1\n").unwrap();
        std::fs::write(
            dir.path().join("BOARD.md"),
            "## Doing\n\n- **VB-12** Harden the login flow\n  - link: auth.py\n\n## Done\n",
        )
        .unwrap();
        git_t(dir.path(), &["add", "."]);
        git_t(dir.path(), &["commit", "-qm", "base"]);
        std::fs::write(dir.path().join("auth.py"), "def login():\n    return 2\n").unwrap();

        let project = fixture(dir.path());
        let s = suggest_commit_message(&project).unwrap();
        assert_eq!(s.resolves, vec!["VB-12"]);
        // Single resolved task titles the subject.
        assert!(s.subject.contains("harden the login flow"), "{:?}", s);
        assert!(s.message.contains("Resolves VB-12"), "{}", s.message);

        let line = perform_commit(&project, &s).unwrap();
        assert!(line.contains("harden the login flow"), "{line}");

        // The task moved to Done and BOARD.md rode along in the commit.
        let b = board::load_board(dir.path(), "commit-demo").unwrap();
        assert_eq!(b.find_task("VB-12").unwrap().0, "Done");
        // -uno: the fixture has no .gitignore, so the .void-stack/ scratch
        // dir the graph opens would show as untracked noise.
        let status = Command::new("git")
            .args([
                "-C",
                &dir.path().to_string_lossy(),
                "status",
                "--porcelain",
                "-uno",
            ])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&status.stdout).trim().is_empty(),
            "working tree must be clean after void commit"
        );
    }

    #[test]
    fn test_suggest_errors_on_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        git_t(dir.path(), &["init", "-q"]);
        git_t(dir.path(), &["config", "user.email", "t@t"]);
        git_t(dir.path(), &["config", "user.name", "t"]);
        git_t(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        git_t(dir.path(), &["add", "."]);
        git_t(dir.path(), &["commit", "-qm", "base"]);
        assert!(suggest_commit_message(&fixture(dir.path())).is_err());
    }
}
