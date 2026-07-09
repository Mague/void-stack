//! `session_context`: one-call session bootstrap for LLM agents.
//!
//! Consolidates what today takes 4-5 tool calls at session start: index
//! stats + structural-graph freshness, a docs digest (README/CLAUDE.md),
//! the current git diff with its affected symbols, the impact radius of
//! the changed files, and the Doing tasks from BOARD.md. Output is compact
//! markdown (~2k tokens hard cap) ready to paste as initial agent context.
//! Every section degrades to an explanatory "n/a" line instead of failing
//! the whole call.

use std::path::{Path, PathBuf};

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// ~2,000 tokens at ~4 chars/token.
const MAX_CONTEXT_CHARS: usize = 8_000;
const DOC_LINES: usize = 30;
const DOC_LINE_WIDTH: usize = 120;
const MAX_DIFF_FILES: usize = 15;
const MAX_DIFF_SYMBOLS: usize = 20;
const MAX_IMPACTED_FILES: usize = 12;

/// Build the session-context markdown for a project.
pub fn session_context(project: &Project) -> Result<String, String> {
    let root = PathBuf::from(strip_win_prefix(&project.path));

    let mut md = format!("# Session context — {}\n\n", project.name);
    md.push_str(&format!("- path: `{}`\n", project.path));
    if let Some(pt) = project.project_type {
        md.push_str(&format!("- type: {:?}\n", pt));
    }
    if !project.tags.is_empty() {
        md.push_str(&format!("- tags: {}\n", project.tags.join(", ")));
    }
    if !project.services.is_empty() {
        let enabled = project.services.iter().filter(|s| s.enabled).count();
        md.push_str(&format!(
            "- services: {} ({} enabled)\n",
            project.services.len(),
            enabled
        ));
    }

    md.push_str(&index_section(project));
    md.push_str(&graph_section(project));
    md.push_str(&docs_section(&root));
    md.push_str(&diff_section(project, &root));
    md.push_str(&board_section(&root, &project.name));

    if md.len() > MAX_CONTEXT_CHARS {
        md.truncate(MAX_CONTEXT_CHARS - 60);
        md.push_str("\n…(truncated to fit the ~2k-token context budget)\n");
    }
    Ok(md)
}

fn age_of(ts: chrono::DateTime<chrono::Utc>) -> String {
    let age = chrono::Utc::now().signed_duration_since(ts);
    if age.num_days() >= 1 {
        format!("{}d ago", age.num_days())
    } else if age.num_hours() >= 1 {
        format!("{}h ago", age.num_hours())
    } else {
        format!("{}m ago", age.num_minutes().max(0))
    }
}

#[cfg(feature = "vector")]
fn index_section(project: &Project) -> String {
    match crate::vector_index::get_index_stats(project) {
        Ok(Some(stats)) => format!(
            "\n## Semantic index\n- {} files, {} chunks, built {} — semantic_search available\n",
            stats.files_indexed,
            stats.chunks_total,
            age_of(stats.created_at)
        ),
        Ok(None) => "\n## Semantic index\n- none — run index_project_codebase first\n".to_string(),
        Err(e) => format!("\n## Semantic index\n- n/a ({})\n", e),
    }
}

#[cfg(not(feature = "vector"))]
fn index_section(_project: &Project) -> String {
    "\n## Semantic index\n- n/a (built without the vector feature)\n".to_string()
}

#[cfg(feature = "structural")]
fn graph_section(project: &Project) -> String {
    let path = crate::structural::structural_db_path(project);
    if !path.exists() {
        return "\n## Structural graph\n- none — run build_structural_graph first\n".to_string();
    }
    let freshness = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| age_of(chrono::DateTime::<chrono::Utc>::from(t)))
        .unwrap_or_else(|_| "unknown age".to_string());
    match crate::structural::open_db(project) {
        Ok(conn) => {
            let nodes = crate::structural::count_nodes(&conn).unwrap_or(0);
            let edges = crate::structural::count_edges(&conn).unwrap_or(0);
            format!(
                "\n## Structural graph\n- {} nodes, {} edges, updated {}\n",
                nodes, edges, freshness
            )
        }
        Err(e) => format!("\n## Structural graph\n- n/a ({})\n", e),
    }
}

#[cfg(not(feature = "structural"))]
fn graph_section(_project: &Project) -> String {
    "\n## Structural graph\n- n/a (built without the structural feature)\n".to_string()
}

/// First `DOC_LINES` non-empty lines of README.md and CLAUDE.md.
fn docs_section(root: &Path) -> String {
    let mut out = String::new();
    for name in ["README.md", "CLAUDE.md"] {
        let Ok(content) = crate::file_reader::read_project_file(root, name) else {
            continue;
        };
        let lines: Vec<String> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(DOC_LINES)
            .map(|l| {
                let mut l = l.to_string();
                if l.len() > DOC_LINE_WIDTH {
                    // Truncate on a char boundary.
                    let mut cut = DOC_LINE_WIDTH;
                    while !l.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    l.truncate(cut);
                    l.push('…');
                }
                l
            })
            .collect();
        if !lines.is_empty() {
            out.push_str(&format!("\n## {} (first lines)\n", name));
            out.push_str(&lines.join("\n"));
            out.push('\n');
        }
    }
    out
}

fn diff_section(project: &Project, root: &Path) -> String {
    let hunks = crate::diff::get_changed_hunks(root, None);
    if hunks.is_empty() {
        return "\n## Current diff (vs HEAD)\n- clean working tree\n".to_string();
    }
    let mut out = format!("\n## Current diff (vs HEAD) — {} files\n", hunks.len());
    for h in hunks.iter().take(MAX_DIFF_FILES) {
        out.push_str(&format!(
            "- {:?} `{}` (+{} / -{})\n",
            h.status, h.file, h.added, h.removed
        ));
    }
    if hunks.len() > MAX_DIFF_FILES {
        out.push_str(&format!(
            "- (+{} more files)\n",
            hunks.len() - MAX_DIFF_FILES
        ));
    }

    #[cfg(feature = "structural")]
    {
        if let Ok(conn) = crate::structural::open_db(project) {
            let symbols = crate::diff::hunks_to_symbols(&conn, &hunks);
            let named: Vec<&crate::diff::ChangedSymbol> =
                symbols.iter().filter(|s| s.kind != "file").collect();
            if !named.is_empty() {
                out.push_str(&format!("\n### Affected symbols ({})\n", named.len()));
                for s in named.iter().take(MAX_DIFF_SYMBOLS) {
                    out.push_str(&format!(
                        "- {} `{}` — {}:{}\n",
                        s.kind, s.name, s.file, s.line_start
                    ));
                }
                if named.len() > MAX_DIFF_SYMBOLS {
                    out.push_str(&format!("- (+{} more)\n", named.len() - MAX_DIFF_SYMBOLS));
                }
            }

            let changed_files: Vec<String> = hunks.iter().map(|h| h.file.clone()).collect();
            match crate::structural::get_impact_radius(&conn, &changed_files, 2, 60, true) {
                Ok(impact) => {
                    let external: Vec<&String> = impact
                        .impacted_files
                        .iter()
                        .filter(|f| !changed_files.contains(f))
                        .collect();
                    out.push_str(&format!(
                        "\n### Impact radius (depth 2) — {} external files\n",
                        external.len()
                    ));
                    for f in external.iter().take(MAX_IMPACTED_FILES) {
                        out.push_str(&format!("- `{}`\n", f));
                    }
                    if external.len() > MAX_IMPACTED_FILES {
                        out.push_str(&format!(
                            "- (+{} more)\n",
                            external.len() - MAX_IMPACTED_FILES
                        ));
                    }
                }
                Err(e) => out.push_str(&format!("\n### Impact radius\n- n/a ({})\n", e)),
            }
        }
    }
    #[cfg(not(feature = "structural"))]
    let _ = project;

    out
}

fn board_section(root: &Path, project_name: &str) -> String {
    let Ok(board) = crate::board::load_board(root, project_name) else {
        return String::new();
    };
    let mut out = String::new();
    for col in &board.columns {
        if !col.name.eq_ignore_ascii_case("Doing") || col.tasks.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## Board — Doing ({})\n", col.tasks.len()));
        for t in &col.tasks {
            out.push_str(&format!("- **{}** {}", t.id, t.title));
            if !t.links.is_empty() {
                out.push_str(&format!(" → {}", t.links.join(", ")));
            }
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success(), "git {:?}: {:?}", args, st);
    }

    fn fixture_project(dir: &Path, name: &str) -> Project {
        Project {
            name: name.to_string(),
            path: dir.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    #[test]
    fn test_session_context_smoke() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("README.md"), "# Demo\n\nA test project.\n").unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(
            dir.path().join("BOARD.md"),
            "## Doing\n\n- **VB-4** Ship the context tool\n  - link: a.rs\n",
        )
        .unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "base"]);
        // Dirty the tree so the diff section has content.
        std::fs::write(dir.path().join("a.rs"), "fn a() { let _x = 1; }\n").unwrap();

        let project = fixture_project(dir.path(), "ctx-demo");
        let md = session_context(&project).unwrap();

        assert!(md.contains("# Session context — ctx-demo"));
        assert!(md.contains("## Semantic index"));
        assert!(md.contains("## Structural graph"));
        assert!(md.contains("## README.md (first lines)"));
        assert!(md.contains("A test project."));
        assert!(md.contains("## Current diff"));
        assert!(md.contains("a.rs"));
        assert!(md.contains("## Board — Doing (1)"));
        assert!(md.contains("VB-4"));
        assert!(md.len() <= MAX_CONTEXT_CHARS);
    }

    #[test]
    fn test_session_context_clean_tree_and_no_docs() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "base"]);

        let project = fixture_project(dir.path(), "ctx-clean");
        let md = session_context(&project).unwrap();
        assert!(md.contains("clean working tree"));
        assert!(!md.contains("README.md (first lines)"));
        assert!(!md.contains("## Board")); // empty Doing column adds nothing
    }

    #[test]
    fn test_session_context_truncates() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        // Docs at their per-line caps plus an oversized Doing column —
        // together they must blow the overall budget.
        let long = (0..DOC_LINES)
            .map(|i| format!("line {} {}", i, "x".repeat(400)))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(dir.path().join("README.md"), &long).unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), &long).unwrap();
        let mut board = String::from("## Doing\n\n");
        for i in 0..200 {
            board.push_str(&format!(
                "- **VB-{}** A fairly long in-flight task title {}\n",
                i + 1,
                i
            ));
        }
        std::fs::write(dir.path().join("BOARD.md"), board).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "base"]);

        let project = fixture_project(dir.path(), "ctx-long");
        let md = session_context(&project).unwrap();
        assert!(md.len() <= MAX_CONTEXT_CHARS);
        assert!(md.contains("truncated"));
    }
}
