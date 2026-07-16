//! Work timeline: the repo's git history and the board's task history,
//! bucketed by period (day / week / month / year) or by conventional-
//! commit dimension (type / scope).
//!
//! Every commit ever made is a work item — parsed as a conventional
//! commit when it is one (`feat(board): ...`), kept verbatim otherwise —
//! and every board task lands in the bucket of its last committed
//! transition. Weeks are ISO weeks, which is as close to "sprints" as
//! git history gets without inventing metadata.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use crate::boardhistory::{self, UNCOMMITTED};
use crate::git_util::git_output as git;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    Day,
    Week,
    Month,
    Year,
    /// Conventional-commit type (feat, fix, docs, ...).
    Type,
    /// Conventional-commit scope — the closest git gets to "feature area".
    Scope,
}

impl GroupBy {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "day" => Ok(GroupBy::Day),
            "week" | "sprint" => Ok(GroupBy::Week),
            "month" => Ok(GroupBy::Month),
            "year" => Ok(GroupBy::Year),
            "type" => Ok(GroupBy::Type),
            "scope" | "area" => Ok(GroupBy::Scope),
            other => Err(format!(
                "unknown grouping '{}' (day, week, month, year, type, scope)",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WorkItem {
    pub commit: String,
    /// Committer date, `YYYY-MM-DD`.
    pub date: String,
    pub author: String,
    /// Conventional-commit type when the subject parses as one.
    pub ctype: Option<String>,
    pub scope: Option<String>,
    pub subject: String,
    /// Board task ids referenced by `Resolves VB-n` lines in the body.
    pub resolves: Vec<String>,
}

/// A board task placed on the timeline by its last committed transition.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TimelineTask {
    pub id: String,
    pub title: String,
    /// Column reached in that transition (e.g. Done).
    pub column: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TimelineBucket {
    /// `2026-07-09` / `2026-W28` / `2026-07` / `2026` / `feat` / `board`.
    pub key: String,
    pub tasks: Vec<TimelineTask>,
    pub commits: Vec<WorkItem>,
}

fn conventional_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([a-z]+)(?:\(([^)]*)\))?!?:\s*(.+)$").unwrap())
}

fn resolves_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?mi)^\s*(?:resolves|closes|fixes)\s+([A-Za-z]+-\d+)").unwrap())
}

/// Every commit in the repo (newest first), optionally bounded by a
/// `--since` expression git understands ("2026-01-01", "3 months ago").
pub fn work_log(project_root: &Path, since: Option<&str>) -> Result<Vec<WorkItem>, String> {
    // \x1f separates fields, \x1e separates records — commit bodies may
    // contain anything line-based formats would choke on.
    let mut args = vec![
        "log",
        "--no-merges",
        "--format=%h%x1f%cs%x1f%an%x1f%s%x1f%b%x1e",
    ];
    let since_arg;
    if let Some(s) = since {
        since_arg = format!("--since={}", s);
        args.push(&since_arg);
    }
    let out = git(project_root, &args)?;
    let mut items = Vec::new();
    for record in out.split('\x1e') {
        let record = record.trim_matches(['\n', ' ']);
        if record.is_empty() {
            continue;
        }
        let mut f = record.split('\x1f');
        let (Some(commit), Some(date), Some(author), Some(subject)) =
            (f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        let body = f.next().unwrap_or("");
        let (ctype, scope, subject) = match conventional_re().captures(subject) {
            Some(c) => (
                Some(c[1].to_string()),
                c.get(2).map(|m| m.as_str().to_string()),
                c[3].to_string(),
            ),
            None => (None, None, subject.to_string()),
        };
        let resolves = resolves_re()
            .captures_iter(body)
            .map(|c| c[1].to_uppercase())
            .collect();
        items.push(WorkItem {
            commit: commit.to_string(),
            date: date.to_string(),
            author: author.to_string(),
            ctype,
            scope,
            subject,
            resolves,
        });
    }
    Ok(items)
}

fn period_key(date: &str, by: GroupBy) -> String {
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok();
    match (by, parsed) {
        (GroupBy::Day, _) => date.to_string(),
        (GroupBy::Week, Some(d)) => {
            let w = chrono::Datelike::iso_week(&d);
            format!("{}-W{:02}", w.year(), w.week())
        }
        (GroupBy::Month, _) => date.chars().take(7).collect(),
        (GroupBy::Year, _) => date.chars().take(4).collect(),
        (_, _) => date.to_string(),
    }
}

fn commit_key(item: &WorkItem, by: GroupBy) -> String {
    match by {
        GroupBy::Type => item.ctype.clone().unwrap_or_else(|| "(other)".into()),
        GroupBy::Scope => item.scope.clone().unwrap_or_else(|| "(none)".into()),
        _ => period_key(&item.date, by),
    }
}

/// The whole timeline: every commit ever made plus every board task ever
/// committed, grouped by `by`. Period buckets sort newest first; type and
/// scope buckets sort by commit count. Tasks only join period buckets —
/// they carry no conventional type or scope.
pub fn board_timeline(
    project_root: &Path,
    project: &str,
    by: GroupBy,
    since: Option<&str>,
) -> Result<Vec<TimelineBucket>, String> {
    let items = work_log(project_root, since)?;
    let mut buckets: BTreeMap<String, TimelineBucket> = BTreeMap::new();

    for item in items {
        let key = commit_key(&item, by);
        buckets
            .entry(key.clone())
            .or_insert_with(|| TimelineBucket {
                key,
                tasks: Vec::new(),
                commits: Vec::new(),
            })
            .commits
            .push(item);
    }

    if !matches!(by, GroupBy::Type | GroupBy::Scope) {
        for h in boardhistory::board_history(project_root, project)? {
            // Last committed transition places the task on the timeline;
            // tasks only ever seen uncommitted have no date to bucket by.
            let Some(e) = h.events.iter().rev().find(|e| e.commit != UNCOMMITTED) else {
                continue;
            };
            let key = period_key(&e.date, by);
            buckets
                .entry(key.clone())
                .or_insert_with(|| TimelineBucket {
                    key,
                    tasks: Vec::new(),
                    commits: Vec::new(),
                })
                .tasks
                .push(TimelineTask {
                    id: h.id.clone(),
                    title: h.title.clone(),
                    column: e.column.clone(),
                    date: e.date.clone(),
                });
        }
    }

    let mut out: Vec<TimelineBucket> = buckets.into_values().collect();
    match by {
        // Periods: newest first (keys are lexicographically ordered dates).
        GroupBy::Day | GroupBy::Week | GroupBy::Month | GroupBy::Year => {
            out.sort_by(|a, b| b.key.cmp(&a.key))
        }
        // Dimensions: busiest first.
        GroupBy::Type | GroupBy::Scope => out.sort_by(|a, b| {
            b.commits
                .len()
                .cmp(&a.commits.len())
                .then_with(|| a.key.cmp(&b.key))
        }),
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CommitFile {
    pub path: String,
    /// `-1` for binary files (git prints `-` in numstat).
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CommitDetail {
    pub commit: String,
    pub full_hash: String,
    pub date: String,
    pub author: String,
    pub ctype: Option<String>,
    pub scope: Option<String>,
    pub subject: String,
    /// Full message body (subject excluded), trimmed.
    pub body: String,
    pub resolves: Vec<String>,
    pub files: Vec<CommitFile>,
    pub additions: i64,
    pub deletions: i64,
}

/// Full detail of one commit: parsed conventional header, message body,
/// resolved task ids and per-file numstat.
pub fn commit_detail(project_root: &Path, hash: &str) -> Result<CommitDetail, String> {
    // The hash reaches git as an argument — accept only hex so nothing
    // flag- or revision-expression-shaped sneaks through.
    if hash.len() < 4 || hash.len() > 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("'{}' is not a commit hash", hash));
    }
    let out = git(
        project_root,
        &[
            "show",
            "--numstat",
            "--format=%h%x1f%H%x1f%cs%x1f%an%x1f%s%x1f%b%x1e",
            hash,
        ],
    )?;
    let (header, stats) = out
        .split_once('\x1e')
        .ok_or_else(|| format!("unexpected git show output for {}", hash))?;
    let mut f = header.split('\x1f');
    let (Some(commit), Some(full), Some(date), Some(author), Some(subject)) =
        (f.next(), f.next(), f.next(), f.next(), f.next())
    else {
        return Err(format!("unexpected git show header for {}", hash));
    };
    let body = f.next().unwrap_or("").trim().to_string();
    let (ctype, scope, subject) = match conventional_re().captures(subject) {
        Some(c) => (
            Some(c[1].to_string()),
            c.get(2).map(|m| m.as_str().to_string()),
            c[3].to_string(),
        ),
        None => (None, None, subject.to_string()),
    };
    let resolves = resolves_re()
        .captures_iter(&body)
        .map(|c| c[1].to_uppercase())
        .collect();
    let mut files = Vec::new();
    let (mut additions, mut deletions) = (0i64, 0i64);
    for line in stats.lines() {
        let mut cols = line.split('\t');
        let (Some(a), Some(d), Some(path)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let a = a.trim().parse::<i64>().unwrap_or(-1);
        let d = d.trim().parse::<i64>().unwrap_or(-1);
        if a >= 0 {
            additions += a;
        }
        if d >= 0 {
            deletions += d;
        }
        files.push(CommitFile {
            path: path.to_string(),
            additions: a,
            deletions: d,
        });
    }
    Ok(CommitDetail {
        commit: commit.to_string(),
        full_hash: full.to_string(),
        date: date.to_string(),
        author: author.to_string(),
        ctype,
        scope,
        subject,
        body,
        resolves,
        files,
        additions,
        deletions,
    })
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

    fn commit(root: &Path, file: &str, msg: &str) {
        std::fs::write(root.join(file), msg).unwrap();
        sh_git(root, &["add", "."]);
        sh_git(root, &["commit", "-q", "-m", msg]);
    }

    #[test]
    fn test_work_log_parses_conventional_and_resolves() {
        let (_tmp, root) = repo();
        commit(&root, "a.rs", "feat(board): add kanban");
        commit(&root, "b.rs", "plain message, not conventional");
        commit(
            &root,
            "c.rs",
            "fix(sync): filter fixtures\n\nDetails here.\n\nResolves VB-28",
        );

        let items = work_log(&root, None).unwrap();
        assert_eq!(items.len(), 3);
        // Newest first.
        assert_eq!(items[0].ctype.as_deref(), Some("fix"));
        assert_eq!(items[0].scope.as_deref(), Some("sync"));
        assert_eq!(items[0].subject, "filter fixtures");
        assert_eq!(items[0].resolves, vec!["VB-28"]);
        assert_eq!(items[1].ctype, None);
        assert_eq!(items[1].subject, "plain message, not conventional");
        assert_eq!(items[2].ctype.as_deref(), Some("feat"));
        assert_eq!(items[2].scope.as_deref(), Some("board"));
    }

    #[test]
    fn test_timeline_groups_by_period_and_includes_tasks() {
        let (_tmp, root) = repo();
        std::fs::write(root.join("BOARD.md"), "## Done\n\n- **VB-1** Shipped\n").unwrap();
        sh_git(&root, &["add", "BOARD.md"]);
        sh_git(&root, &["commit", "-q", "-m", "chore(board): update board"]);
        commit(&root, "a.rs", "feat(core): something");

        // All commits happen "today" in the fixture, so every grouping
        // yields exactly one bucket holding both commits and the task.
        for by in ["day", "week", "month", "year"] {
            let buckets = board_timeline(&root, "demo", GroupBy::parse(by).unwrap(), None).unwrap();
            assert_eq!(buckets.len(), 1, "grouping {}", by);
            assert_eq!(buckets[0].commits.len(), 2);
            assert_eq!(buckets[0].tasks.len(), 1, "grouping {}", by);
            assert_eq!(buckets[0].tasks[0].id, "VB-1");
            assert_eq!(buckets[0].tasks[0].column, "Done");
        }
        // Week keys look like 2026-W28.
        let weeks = board_timeline(&root, "demo", GroupBy::Week, None).unwrap();
        assert!(weeks[0].key.contains("-W"), "week key was {}", weeks[0].key);
    }

    #[test]
    fn test_timeline_groups_by_type_and_scope() {
        let (_tmp, root) = repo();
        commit(&root, "a.rs", "feat(board): one");
        commit(&root, "b.rs", "feat(board): two");
        commit(&root, "c.rs", "fix(sync): three");
        commit(&root, "d.rs", "no convention here");

        let by_type = board_timeline(&root, "demo", GroupBy::Type, None).unwrap();
        let keys: Vec<&str> = by_type.iter().map(|b| b.key.as_str()).collect();
        // Busiest first, unparsed commits under "(other)".
        assert_eq!(keys, vec!["feat", "(other)", "fix"]);
        assert_eq!(by_type[0].commits.len(), 2);
        assert!(by_type.iter().all(|b| b.tasks.is_empty()));

        let by_scope = board_timeline(&root, "demo", GroupBy::Scope, None).unwrap();
        let keys: Vec<&str> = by_scope.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, vec!["board", "(none)", "sync"]);
    }

    #[test]
    fn test_commit_detail_parses_files_and_body() {
        let (_tmp, root) = repo();
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
        let hash = {
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
        };

        let d = commit_detail(&root, &hash).unwrap();
        assert_eq!(d.commit, hash);
        assert_eq!(d.ctype.as_deref(), Some("feat"));
        assert_eq!(d.scope.as_deref(), Some("core"));
        assert_eq!(d.subject, "add things");
        assert!(d.body.contains("Longer explanation."));
        assert_eq!(d.resolves, vec!["VB-7"]);
        assert_eq!(d.files.len(), 2);
        assert_eq!(d.additions, 3);
        assert_eq!(d.deletions, 0);

        // Anything that is not a plain hex hash is rejected.
        assert!(commit_detail(&root, "HEAD").is_err());
        assert!(commit_detail(&root, "--help").is_err());
        assert!(commit_detail(&root, &format!("{}^", hash)).is_err());
    }

    #[test]
    fn test_group_by_parse_aliases() {
        assert_eq!(GroupBy::parse("sprint").unwrap(), GroupBy::Week);
        assert_eq!(GroupBy::parse("area").unwrap(), GroupBy::Scope);
        assert!(GroupBy::parse("quarter").is_err());
    }
}
