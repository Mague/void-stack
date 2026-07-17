//! Board tools: kanban board stored as BOARD.md in the managed repo.

use std::path::PathBuf;

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::board;
use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

use crate::types::{
    BoardAddTaskRequest, BoardHistoryRequest, BoardLinkTaskRequest, BoardMoveTaskRequest,
    BoardTimelineRequest, CommitDetailRequest,
};

fn root_of(project: &Project) -> PathBuf {
    PathBuf::from(strip_win_prefix(&project.path))
}

fn load(project: &Project) -> Result<(board::Board, PathBuf), McpError> {
    let root = root_of(project);
    let b =
        board::load_board(&root, &project.name).map_err(|e| McpError::internal_error(e, None))?;
    Ok((b, root))
}

fn save(root: &std::path::Path, b: &board::Board) -> Result<(), McpError> {
    board::save_board(root, b).map_err(|e| McpError::internal_error(e, None))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Logic for board_list tool — returns the canonical markdown, which is
/// both the storage format and a fine LLM payload.
pub fn board_list(project: &Project) -> Result<CallToolResult, McpError> {
    let (b, root) = load(project)?;
    let mut out = board::board_to_markdown(&b);
    out.push_str(&format!(
        "\n_(file: {})_\n",
        board::board_path(&root).display()
    ));
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Logic for board_add_task tool.
pub fn board_add_task(
    project: &Project,
    req: &BoardAddTaskRequest,
) -> Result<CallToolResult, McpError> {
    let (mut b, root) = load(project)?;
    let tags = req.tags.clone().unwrap_or_default();
    let id = board::add_task(&mut b, &req.title, req.priority.as_deref(), &tags, &today());
    save(&root, &b)?;
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Task {} added to Backlog: {}",
        id, req.title
    ))]))
}

/// Logic for board_move_task tool.
pub fn board_move_task(
    project: &Project,
    req: &BoardMoveTaskRequest,
) -> Result<CallToolResult, McpError> {
    let (mut b, root) = load(project)?;
    board::move_task(&mut b, &req.id, &req.column)
        .map_err(|e| McpError::invalid_params(e, None))?;
    save(&root, &b)?;
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Task {} moved to {}",
        req.id.to_uppercase(),
        req.column
    ))]))
}

/// Logic for sync_todos tool. Scans the whole tree, so callers run it on
/// a blocking thread.
pub fn sync_todos(project: &Project, clean: bool) -> Result<CallToolResult, McpError> {
    let report = void_stack_core::todosync::sync_todos_with(project, clean)
        .map_err(|e| McpError::internal_error(e, None))?;
    let mut out = format!(
        "todo-sync: {} marker(s) in code — {} added, {} unchanged, {} resolved, {} purged",
        report.markers_found, report.added, report.unchanged, report.resolved, report.purged
    );
    if !report.added_ids.is_empty() {
        out.push_str(&format!("\nnew tasks: {}", report.added_ids.join(", ")));
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Logic for board_archive_done tool.
pub fn board_archive_done(
    project: &Project,
    days: Option<i64>,
) -> Result<CallToolResult, McpError> {
    let (mut b, root) = load(project)?;
    let n = board::archive_done(
        &root,
        &mut b,
        days.unwrap_or(14),
        chrono::Local::now().date_naive(),
    )
    .map_err(|e| McpError::internal_error(e, None))?;
    save(&root, &b)?;
    Ok(CallToolResult::success(vec![Content::text(format!(
        "{} task(s) archived to {}",
        n,
        board::ARCHIVE_FILE
    ))]))
}

fn history_status(h: &void_stack_core::boardhistory::TaskHistory) -> String {
    match (&h.current_column, h.archived) {
        (Some(col), _) => col.clone(),
        (None, true) => "archived".into(),
        (None, false) => "removed".into(),
    }
}

fn history_markdown(h: &void_stack_core::boardhistory::TaskHistory) -> String {
    let mut out = format!("## {} — {} [{}]\n", h.id, h.title, history_status(h));
    if let Some(p) = &h.priority {
        out.push_str(&format!("- priority: {}\n", p));
    }
    if !h.tags.is_empty() {
        out.push_str(&format!(
            "- tags: {}\n",
            h.tags
                .iter()
                .map(|t| format!("#{}", t))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if let Some(d) = &h.date {
        out.push_str(&format!("- created: {}\n", d));
    }
    for link in &h.links {
        out.push_str(&format!("- link: {}\n", link));
    }
    if !h.events.is_empty() {
        out.push_str("- timeline:\n");
        for e in &h.events {
            let when = if e.date.is_empty() {
                String::new()
            } else {
                format!(" {}", e.date)
            };
            out.push_str(&format!("  - {}{} → {}\n", e.commit, when, e.column));
        }
    }
    out
}

/// Logic for board_history tool. Walks the git log of BOARD.md, so
/// callers run it on a blocking thread.
pub fn board_history(
    project: &Project,
    req: &BoardHistoryRequest,
) -> Result<CallToolResult, McpError> {
    let root = root_of(project);
    let out = match req.id.as_deref() {
        Some(id) => {
            let h = void_stack_core::boardhistory::task_history(&root, &project.name, id)
                .map_err(|e| McpError::invalid_params(e, None))?;
            history_markdown(&h)
        }
        None => {
            let hist = void_stack_core::boardhistory::board_history(&root, &project.name)
                .map_err(|e| McpError::internal_error(e, None))?;
            let mut out = format!(
                "# Board history — {} ({} task(s) ever)\n\n",
                project.name,
                hist.len()
            );
            for h in &hist {
                out.push_str(&history_markdown(h));
                out.push('\n');
            }
            out
        }
    };
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Logic for board_timeline tool. Walks the whole git log, so callers
/// run it on a blocking thread.
pub fn board_timeline(
    project: &Project,
    req: &BoardTimelineRequest,
) -> Result<CallToolResult, McpError> {
    let root = root_of(project);
    let by = req.by.as_deref().unwrap_or("month");
    let group = void_stack_core::timeline::GroupBy::parse(by)
        .map_err(|e| McpError::invalid_params(e, None))?;
    let buckets = void_stack_core::timeline::board_timeline(
        &root,
        &project.name,
        group,
        req.since.as_deref(),
    )
    .map_err(|e| McpError::internal_error(e, None))?;
    let mut out = format!("# Timeline — {} (by {})\n\n", project.name, by);
    for b in &buckets {
        out.push_str(&format!(
            "## {} — {} commit(s), {} task(s)\n",
            b.key,
            b.commits.len(),
            b.tasks.len()
        ));
        for t in &b.tasks {
            out.push_str(&format!("- task {} [{}] {}\n", t.id, t.column, t.title));
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
                format!(" [{}]", c.resolves.join(", "))
            };
            out.push_str(&format!(
                "- {} {} {} {}{}\n",
                c.commit, c.date, kind, c.subject, resolves
            ));
        }
        out.push('\n');
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Logic for commit_detail tool.
pub fn commit_detail(
    project: &Project,
    req: &CommitDetailRequest,
) -> Result<CallToolResult, McpError> {
    let root = root_of(project);
    let d = void_stack_core::timeline::commit_detail(&root, &req.hash)
        .map_err(|e| McpError::invalid_params(e, None))?;
    let kind = match (&d.ctype, &d.scope) {
        (Some(t), Some(s)) => format!("{}({})", t, s),
        (Some(t), None) => t.clone(),
        _ => "-".into(),
    };
    let mut out = format!(
        "# {} — {} {}\n\n- date: {}\n- author: {}\n- hash: {}\n- changes: +{} / -{}\n",
        d.commit, kind, d.subject, d.date, d.author, d.full_hash, d.additions, d.deletions
    );
    if !d.resolves.is_empty() {
        out.push_str(&format!("- resolves: {}\n", d.resolves.join(", ")));
    }
    if !d.body.is_empty() {
        out.push_str(&format!("\n{}\n", d.body));
    }
    out.push_str("\n## Files\n");
    for f in &d.files {
        out.push_str(&format!(
            "- {} (+{} / -{})\n",
            f.path, f.additions, f.deletions
        ));
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Logic for board_link_task tool. With the vector feature the query is
/// resolved through the semantic index to concrete files; without it (or
/// when the query already looks like a path/symbol) it is linked verbatim.
pub fn board_link_task(
    project: &Project,
    req: &BoardLinkTaskRequest,
) -> Result<CallToolResult, McpError> {
    let (mut b, root) = load(project)?;

    let mut links: Vec<String> = Vec::new();
    let mut resolved_via_index = false;

    // Path-like or symbol-like queries link directly, no search needed.
    let literal = req.query.contains('/') || req.query.contains("::") || req.query.contains('.');

    #[cfg(feature = "vector")]
    if !literal
        && let Ok(results) = void_stack_core::vector_index::semantic_search(project, &req.query, 3)
    {
        for r in &results {
            if !links.contains(&r.file_path) {
                links.push(r.file_path.clone());
            }
        }
        resolved_via_index = !links.is_empty();
    }

    if links.is_empty() {
        links.push(req.query.trim().to_string());
    }

    board::link_task(&mut b, &req.id, &links).map_err(|e| McpError::invalid_params(e, None))?;
    save(&root, &b)?;

    let how = if resolved_via_index {
        "resolved via semantic index"
    } else {
        "linked verbatim"
    };
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Task {} linked ({}): {}",
        req.id.to_uppercase(),
        how,
        links.join(", ")
    ))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;
    use void_stack_core::boardhistory::{TaskEvent, TaskHistory};

    /// Extract the text payload of a successful tool result.
    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    /// Project fixture pointing at a tempdir root.
    fn project_at(root: &Path) -> Project {
        Project {
            name: "demo".to_string(),
            description: String::new(),
            path: root.to_string_lossy().to_string(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

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

    /// Tempdir git repo with the usual test-safe config.
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

    fn short_head(root: &Path) -> String {
        let out = Command::new("git")
            .args([
                "-C",
                &root.to_string_lossy(),
                "rev-parse",
                "--short",
                "HEAD",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    // ── board_list / board_add_task / board_move_task ───────

    #[test]
    fn test_board_list_renders_markdown_and_file_path() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_at(tmp.path());
        std::fs::write(
            tmp.path().join("BOARD.md"),
            "## Backlog\n\n- **VB-1** Fix login `prio:high` `#auth`\n",
        )
        .unwrap();

        let out = text_of(&board_list(&project).unwrap());
        assert!(out.contains("# Void Board — demo"), "got: {out}");
        assert!(out.contains("- **VB-1** Fix login `prio:high` `#auth`"));
        assert!(out.contains("_(file:"), "must show the board file path");
        assert!(out.contains("BOARD.md"));
    }

    #[test]
    fn test_board_add_task_persists_and_reports_id() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_at(tmp.path());
        let req = BoardAddTaskRequest {
            project: "demo".into(),
            title: "Fix login".into(),
            priority: Some("high".into()),
            tags: Some(vec!["auth".into()]),
        };

        let out = text_of(&board_add_task(&project, &req).unwrap());
        assert_eq!(out, "Task VB-1 added to Backlog: Fix login");

        let md = std::fs::read_to_string(tmp.path().join("BOARD.md")).unwrap();
        assert!(md.contains("- **VB-1** Fix login `prio:high` `#auth`"));
        // Creation date is stamped with today.
        assert!(md.contains(&today()), "task line must carry today's date");
    }

    #[test]
    fn test_board_move_task_and_unknown_id() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_at(tmp.path());
        std::fs::write(
            tmp.path().join("BOARD.md"),
            "## Backlog\n\n- **VB-1** Fix login\n\n## Doing\n",
        )
        .unwrap();

        let req = BoardMoveTaskRequest {
            project: "demo".into(),
            id: "vb-1".into(),
            column: "Doing".into(),
        };
        let out = text_of(&board_move_task(&project, &req).unwrap());
        assert_eq!(out, "Task VB-1 moved to Doing");

        let md = std::fs::read_to_string(tmp.path().join("BOARD.md")).unwrap();
        let doing = md.split("## Doing").nth(1).unwrap();
        assert!(doing.contains("- **VB-1** Fix login"));

        let bad = BoardMoveTaskRequest {
            project: "demo".into(),
            id: "VB-99".into(),
            column: "Doing".into(),
        };
        assert!(board_move_task(&project, &bad).is_err());
    }

    #[test]
    fn test_board_link_task_literal_path_links_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_at(tmp.path());
        std::fs::write(
            tmp.path().join("BOARD.md"),
            "## Backlog\n\n- **VB-1** Fix login\n",
        )
        .unwrap();

        // Path-like queries bypass the semantic index entirely.
        let req = BoardLinkTaskRequest {
            project: "demo".into(),
            id: "vb-1".into(),
            query: "src/auth/login.rs".into(),
        };
        let out = text_of(&board_link_task(&project, &req).unwrap());
        assert!(
            out.contains("Task VB-1 linked (linked verbatim)"),
            "got: {out}"
        );
        assert!(out.contains("src/auth/login.rs"));

        let md = std::fs::read_to_string(tmp.path().join("BOARD.md")).unwrap();
        assert!(md.contains("  - link: src/auth/login.rs"));
    }

    #[test]
    fn test_board_archive_done_moves_old_tasks() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_at(tmp.path());
        std::fs::write(
            tmp.path().join("BOARD.md"),
            "## Backlog\n\n## Done\n\n- **VB-1** Ancient work `2020-01-01`\n",
        )
        .unwrap();

        let out = text_of(&board_archive_done(&project, None).unwrap());
        assert_eq!(
            out,
            format!("1 task(s) archived to {}", board::ARCHIVE_FILE)
        );

        let archive = std::fs::read_to_string(tmp.path().join(board::ARCHIVE_FILE)).unwrap();
        assert!(archive.contains("- **VB-1** Ancient work"));
        let md = std::fs::read_to_string(tmp.path().join("BOARD.md")).unwrap();
        assert!(!md.contains("VB-1"), "archived task must leave the board");
    }

    #[test]
    fn test_sync_todos_adds_tasks_from_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_at(tmp.path());
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src").join("main.rs"),
            "fn main() {\n    // TODO: replace the placeholder parser\n}\n",
        )
        .unwrap();

        let out = text_of(&sync_todos(&project, false).unwrap());
        assert!(
            out.contains("1 marker(s) in code — 1 added, 0 unchanged, 0 resolved, 0 purged"),
            "got: {out}"
        );
        assert!(out.contains("new tasks: VB-1"), "got: {out}");
    }

    // ── history formatters ──────────────────────────────────

    fn history_fixture() -> TaskHistory {
        TaskHistory {
            id: "VB-1".into(),
            title: "Fix login".into(),
            priority: Some("high".into()),
            tags: vec!["auth".into(), "ui".into()],
            date: Some("2026-07-01".into()),
            links: vec!["src/auth.rs".into()],
            current_column: Some("Doing".into()),
            archived: false,
            events: vec![
                TaskEvent {
                    commit: "abc1234".into(),
                    date: "2026-07-01".into(),
                    author: "t".into(),
                    column: "Backlog".into(),
                },
                TaskEvent {
                    commit: "def5678".into(),
                    date: "2026-07-02".into(),
                    author: "t".into(),
                    column: "Doing".into(),
                },
            ],
        }
    }

    #[test]
    fn test_history_status_variants() {
        let mut h = history_fixture();
        assert_eq!(history_status(&h), "Doing");

        h.current_column = None;
        h.archived = true;
        assert_eq!(history_status(&h), "archived");

        h.archived = false;
        assert_eq!(history_status(&h), "removed");
    }

    #[test]
    fn test_history_markdown_shape() {
        let out = history_markdown(&history_fixture());
        assert!(
            out.starts_with("## VB-1 — Fix login [Doing]\n"),
            "got: {out}"
        );
        assert!(out.contains("- priority: high\n"));
        assert!(out.contains("- tags: #auth #ui\n"));
        assert!(out.contains("- created: 2026-07-01\n"));
        assert!(out.contains("- link: src/auth.rs\n"));
        assert!(out.contains("- timeline:\n"));
        assert!(out.contains("  - abc1234 2026-07-01 → Backlog\n"));
        assert!(out.contains("  - def5678 2026-07-02 → Doing\n"));
    }

    #[test]
    fn test_history_markdown_omits_empty_sections_and_dates() {
        let mut h = history_fixture();
        h.priority = None;
        h.tags.clear();
        h.date = None;
        h.links.clear();
        h.events = vec![TaskEvent {
            commit: "(uncommitted)".into(),
            date: String::new(),
            author: String::new(),
            column: "Done".into(),
        }];
        let out = history_markdown(&h);
        assert!(!out.contains("- priority:"));
        assert!(!out.contains("- tags:"));
        assert!(!out.contains("- created:"));
        assert!(!out.contains("- link:"));
        // Empty event date leaves no dangling space before the arrow.
        assert!(out.contains("  - (uncommitted) → Done\n"), "got: {out}");
    }

    // ── board_history / board_timeline / commit_detail over git ──

    #[test]
    fn test_board_history_over_git_repo() {
        let (_tmp, root) = repo();
        let project = project_at(&root);
        commit_board(&root, "## Backlog\n\n- **VB-1** Fix login\n", "v1");
        commit_board(
            &root,
            "## Backlog\n\n## Doing\n\n- **VB-1** Fix login\n",
            "v2",
        );

        // Whole-board history.
        let req = BoardHistoryRequest {
            project: "demo".into(),
            id: None,
        };
        let out = text_of(&board_history(&project, &req).unwrap());
        assert!(
            out.contains("# Board history — demo (1 task(s) ever)"),
            "got: {out}"
        );
        assert!(out.contains("## VB-1 — Fix login [Doing]"));
        assert!(out.contains("→ Backlog"));
        assert!(out.contains("→ Doing"));

        // Single-task detail.
        let one = BoardHistoryRequest {
            project: "demo".into(),
            id: Some("vb-1".into()),
        };
        let out = text_of(&board_history(&project, &one).unwrap());
        assert!(out.starts_with("## VB-1 — Fix login [Doing]"));

        // Unknown ids are invalid params.
        let bad = BoardHistoryRequest {
            project: "demo".into(),
            id: Some("VB-99".into()),
        };
        assert!(board_history(&project, &bad).is_err());
    }

    #[test]
    fn test_board_timeline_buckets_commits_and_tasks() {
        let (_tmp, root) = repo();
        let project = project_at(&root);
        std::fs::write(root.join("BOARD.md"), "## Done\n\n- **VB-1** Ship it\n").unwrap();
        sh_git(&root, &["add", "BOARD.md"]);
        sh_git(
            &root,
            &["commit", "-q", "-m", "feat(board): ship\n\nResolves VB-1"],
        );

        // Default grouping is by month.
        let req = BoardTimelineRequest {
            project: "demo".into(),
            by: None,
            since: None,
        };
        let out = text_of(&board_timeline(&project, &req).unwrap());
        let month = chrono::Local::now().format("%Y-%m").to_string();
        assert!(out.contains("# Timeline — demo (by month)"), "got: {out}");
        assert!(out.contains(&format!("## {} — 1 commit(s), 1 task(s)", month)));
        assert!(out.contains("- task VB-1 [Done] Ship it"));
        // Commit line: hash + date + type(scope) + subject + resolved ids.
        let hash = short_head(&root);
        assert!(out.contains(&format!("- {} ", hash)), "got: {out}");
        assert!(out.contains("feat(board) ship [VB-1]"));

        // Non-conventional commits render "-" as their kind.
        commit_board(
            &root,
            "## Done\n\n- **VB-1** Ship it\n\n## Backlog\n",
            "plain msg",
        );
        let out = text_of(&board_timeline(&project, &req).unwrap());
        assert!(out.contains(" - plain msg\n"), "got: {out}");

        // Invalid grouping is rejected.
        let bad = BoardTimelineRequest {
            project: "demo".into(),
            by: Some("quarter".into()),
            since: None,
        };
        assert!(board_timeline(&project, &bad).is_err());
    }

    #[test]
    fn test_commit_detail_formats_full_report() {
        let (_tmp, root) = repo();
        let project = project_at(&root);
        std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(root.join("b.rs"), "fn b() {}\nfn c() {}\n").unwrap();
        sh_git(&root, &["add", "."]);
        sh_git(
            &root,
            &[
                "commit",
                "-q",
                "-m",
                "feat(core): add things\n\nLonger explanation.\n\nResolves VB-7",
            ],
        );
        let hash = short_head(&root);

        let req = CommitDetailRequest {
            project: "demo".into(),
            hash: hash.clone(),
        };
        let out = text_of(&commit_detail(&project, &req).unwrap());
        assert!(
            out.starts_with(&format!("# {} — feat(core) add things", hash)),
            "got: {out}"
        );
        assert!(out.contains("- author: t\n"));
        assert!(out.contains("- changes: +3 / -0\n"));
        assert!(out.contains("- resolves: VB-7\n"));
        assert!(out.contains("Longer explanation."));
        assert!(out.contains("## Files"));
        assert!(out.contains("- a.rs (+1 / -0)"));
        assert!(out.contains("- b.rs (+2 / -0)"));

        // Non-hex "hashes" are rejected before reaching git.
        let bad = CommitDetailRequest {
            project: "demo".into(),
            hash: "--help".into(),
        };
        assert!(commit_detail(&project, &bad).is_err());
    }
}
