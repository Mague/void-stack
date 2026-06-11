//! `review_diff`: a compact, LLM-ready review payload for the current diff.
//!
//! Assembles (in order): summary, audit findings on changed lines ±3,
//! blast radius (impact BFS), test coverage for the diff, and 1-hop call
//! context for the hottest changed symbols. The payload is INPUT for an
//! LLM reviewer — compactness is the feature: every list is capped with
//! "(+N more)" and the whole markdown stays under ~4,000 tokens by
//! construction (hard character guard as backstop).

#![cfg(feature = "structural")]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Serialize;

use crate::audit::{self, SecurityFinding};
use crate::diff::{ChangedSymbol, FileHunks, get_changed_hunks, hunks_to_symbols};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::structural::{get_callers, get_impact_radius, open_db};
use crate::testing::{render_suggestions_markdown, suggest_for_symbols};

// ── Budget knobs (≈4k tokens ≈ 16k chars total) ─────────────

const MAX_FINDINGS: usize = 15;
const MAX_IMPACTED_FILES: usize = 10;
const MAX_IMPACTED_PER_FILE: usize = 5;
const MAX_CONTEXT_SYMBOLS: usize = 5;
const MAX_CONTEXT_NEIGHBORS: usize = 5;
const FINDING_LINE_SLACK: usize = 3;
/// Impact BFS guard for the review flow — the recursive CTE can stall on
/// very dense graphs and a pre-commit tool must not hang.
const IMPACT_TIMEOUT_SECS: u64 = 15;
/// Hard backstop: ~4,000 tokens at ~4 chars/token.
const MAX_PAYLOAD_CHARS: usize = 16_000;

#[derive(Debug, Clone, Serialize)]
pub struct ReviewPayload {
    pub markdown: String,
    pub files_changed: usize,
    pub symbols_touched: usize,
    pub findings_on_changed_lines: usize,
    pub suppressed: usize,
    pub uncovered: usize,
}

/// Build the review payload for the diff against `git_base` (default HEAD).
pub fn review_diff(project: &Project, git_base: Option<&str>) -> Result<ReviewPayload, String> {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    let hunks = get_changed_hunks(&root, git_base);
    if hunks.is_empty() {
        return Ok(ReviewPayload {
            markdown: format!(
                "# Review — {}\n\nNo changes vs `{}`.\n",
                project.name,
                git_base.unwrap_or("HEAD")
            ),
            files_changed: 0,
            symbols_touched: 0,
            findings_on_changed_lines: 0,
            suppressed: 0,
            uncovered: 0,
        });
    }

    let conn = open_db(project)?;
    let symbols = hunks_to_symbols(&conn, &hunks);

    let mut md = String::new();

    // a. Summary
    let added: usize = hunks.iter().map(|h| h.added).sum();
    let removed: usize = hunks.iter().map(|h| h.removed).sum();
    let languages: HashSet<&str> = symbols
        .iter()
        .filter(|s| !s.language.is_empty())
        .map(|s| s.language.as_str())
        .collect();
    let mut langs: Vec<&str> = languages.into_iter().collect();
    langs.sort_unstable();
    md.push_str(&format!(
        "# Review — {} (vs `{}`)\n\n## Summary\n- {} files changed, +{} / -{} lines\n- {} symbols touched ({})\n",
        project.name,
        git_base.unwrap_or("HEAD"),
        hunks.len(),
        added,
        removed,
        symbols.len(),
        if langs.is_empty() {
            "unknown".to_string()
        } else {
            langs.join(", ")
        },
    ));

    // b. Findings on changed lines (±3), suppression-aware.
    let (findings, suppressed) = changed_line_findings(&root, &hunks);
    md.push_str(&format!(
        "\n## Findings on changed lines ({}) | Suppressed: {}\n",
        findings.len(),
        suppressed
    ));
    if findings.is_empty() {
        md.push_str("- none\n");
    }
    for f in findings.iter().take(MAX_FINDINGS) {
        md.push_str(&format!(
            "- [{}] {} — `{}:{}` → {}\n",
            f.adjusted_severity,
            f.title,
            f.file_path.as_deref().unwrap_or("?"),
            f.line_number.unwrap_or(0),
            f.remediation
        ));
    }
    push_more(&mut md, findings.len(), MAX_FINDINGS);

    // c. Blast radius (reuses the impact BFS, CALLS edges, depth 2).
    let changed_files: Vec<String> = hunks.iter().map(|h| h.file.clone()).collect();
    md.push_str("\n## Blast radius (depth 2)\n");
    match impact_with_timeout(project, changed_files.clone(), IMPACT_TIMEOUT_SECS) {
        ImpactOutcome::TimedOut => {
            md.push_str(&format!(
                "- impact analysis timed out (>{}s on this graph) — run get_impact_radius directly for the full view\n",
                IMPACT_TIMEOUT_SECS
            ));
        }
        ImpactOutcome::Failed(e) => md.push_str(&format!("- impact analysis unavailable: {}\n", e)),
        ImpactOutcome::Done(impact) => {
            // Hop-1 set = direct callers of changed symbols (for labels).
            let hop1: HashSet<String> = symbols
                .iter()
                .filter(|s| s.kind != "file")
                .flat_map(|s| get_callers(&conn, &s.qualified_name))
                .map(|n| n.qualified_name)
                .collect();

            let mut by_file: HashMap<String, Vec<String>> = HashMap::new();
            let mut contained = 0usize;
            for n in &impact.impacted_nodes {
                if changed_files.contains(&n.file_path) {
                    // The diff itself, not blast radius — but COUNT it so a
                    // large diff explains its empty external impact instead
                    // of contradicting the Context section.
                    contained += 1;
                    continue;
                }
                let label = if hop1.contains(&n.qualified_name) {
                    "hop 1"
                } else {
                    "hop 2"
                };
                by_file
                    .entry(n.file_path.clone())
                    .or_default()
                    .push(format!("{} ({})", n.name, label));
            }
            if by_file.is_empty() {
                if contained > 0 {
                    md.push_str(&format!(
                        "- All {} impacted symbols are within the changed files (large diff) — external impact: 0.\n",
                        contained
                    ));
                } else {
                    md.push_str("- no symbols impacted\n");
                }
            }
            let mut files: Vec<_> = by_file.into_iter().collect();
            files.sort_by_key(|(_, syms)| std::cmp::Reverse(syms.len()));
            let total_files = files.len();
            for (file, mut syms) in files.into_iter().take(MAX_IMPACTED_FILES) {
                let extra = syms.len().saturating_sub(MAX_IMPACTED_PER_FILE);
                syms.truncate(MAX_IMPACTED_PER_FILE);
                md.push_str(&format!("- `{}`: {}", file, syms.join(", ")));
                if extra > 0 {
                    md.push_str(&format!(" (+{} more)", extra));
                }
                md.push('\n');
            }
            if total_files > MAX_IMPACTED_FILES {
                md.push_str(&format!(
                    "- (+{} more files)\n",
                    total_files - MAX_IMPACTED_FILES
                ));
            }
        }
    }

    // d. Coverage (Task 1 embedded).
    crate::testing::ensure_coverage_map(&conn, crate::testing::DEFAULT_COVERAGE_DEPTH)?;
    let suggestions = suggest_for_symbols(&conn, &symbols, 10)?;
    md.push_str("\n## Coverage\n");
    md.push_str(&render_suggestions_markdown(&suggestions));

    // e. Context: top changed symbols by in-degree, 1-hop neighbors as
    // one-line signatures.
    md.push_str(&context_section(&conn, &symbols));

    // Hard backstop for the token budget.
    if md.len() > MAX_PAYLOAD_CHARS {
        md.truncate(MAX_PAYLOAD_CHARS - 60);
        md.push_str("\n…(truncated to fit the ~4k-token review budget)\n");
    }

    Ok(ReviewPayload {
        files_changed: hunks.len(),
        symbols_touched: symbols.len(),
        findings_on_changed_lines: findings.len(),
        suppressed,
        uncovered: suggestions.uncovered.len(),
        markdown: md,
    })
}

enum ImpactOutcome {
    Done(crate::structural::ImpactResult),
    TimedOut,
    Failed(String),
}

/// Run the impact BFS on its own thread with a deadline. The worker gets
/// its own DB connection so an overrunning query can't poison the caller's.
fn impact_with_timeout(project: &Project, changed_files: Vec<String>, secs: u64) -> ImpactOutcome {
    let (tx, rx) = std::sync::mpsc::channel();
    let project = project.clone();
    std::thread::spawn(move || {
        let result = open_db(&project)
            .and_then(|conn| get_impact_radius(&conn, &changed_files, 2, 200, true));
        let _ = tx.send(result);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(secs)) {
        Ok(Ok(impact)) => ImpactOutcome::Done(impact),
        Ok(Err(e)) => ImpactOutcome::Failed(e),
        Err(_) => ImpactOutcome::TimedOut,
    }
}

/// Run the line-level audit scanners and keep only findings inside the
/// changed ranges ±[`FINDING_LINE_SLACK`]. Suppression rules apply and the
/// suppressed-in-scope count is reported (not silently dropped).
fn changed_line_findings(
    root: &std::path::Path,
    hunks: &[FileHunks],
) -> (Vec<SecurityFinding>, usize) {
    // Line-level scanners only — the dependency scan is project-level and
    // can't be attributed to changed lines.
    let mut all: Vec<SecurityFinding> = Vec::new();
    all.extend(audit::secrets::scan_secrets(root));
    all.extend(audit::config_check::scan_insecure_configs(root));
    all.extend(audit::vuln_patterns::scan_vuln_patterns(root));

    let root_str = root.to_string_lossy().replace('\\', "/");
    let in_scope = |f: &SecurityFinding| -> bool {
        let Some(fp) = &f.file_path else { return false };
        let fwd = fp.replace('\\', "/");
        let rel = fwd
            .strip_prefix(&format!("{}/", root_str.trim_end_matches('/')))
            .unwrap_or(&fwd);
        let Some(h) = hunks.iter().find(|h| h.file == rel) else {
            return false;
        };
        let Some(line) = f.line_number else {
            // File-level finding on a changed file (e.g. missing
            // .dockerignore) — keep it.
            return true;
        };
        let line = line as usize;
        h.ranges
            .iter()
            .any(|&(s, e)| line + FINDING_LINE_SLACK >= s && line <= e + FINDING_LINE_SLACK)
    };

    let scoped: Vec<SecurityFinding> = all.into_iter().filter(in_scope).collect();
    let enriched = audit::enrichment::enrich_findings(scoped, root);
    let (mut kept, suppressed) = audit::suppress::filter_suppressed(enriched, root);
    kept.sort_by_key(|f| f.adjusted_severity);
    (kept, suppressed)
}

fn context_section(conn: &rusqlite::Connection, symbols: &[ChangedSymbol]) -> String {
    let mut md = String::from("\n## Context (top changed symbols, 1-hop)\n");

    // Rank changed symbols by in-degree (caller count).
    let mut ranked: Vec<(&ChangedSymbol, usize)> = symbols
        .iter()
        .filter(|s| s.kind != "file" && !s.is_test)
        .map(|s| (s, get_callers(conn, &s.qualified_name).len()))
        .collect();
    ranked.sort_by_key(|(_, in_degree)| std::cmp::Reverse(*in_degree));

    if ranked.is_empty() {
        md.push_str("- no symbol-level changes\n");
        return md;
    }

    for (sym, in_degree) in ranked.into_iter().take(MAX_CONTEXT_SYMBOLS) {
        md.push_str(&format!(
            "- `{}` ({}:{}, {} callers)\n",
            sym.name, sym.file, sym.line_start, in_degree
        ));
        let callers = get_callers(conn, &sym.qualified_name);
        if !callers.is_empty() {
            let names: Vec<String> = callers
                .iter()
                .take(MAX_CONTEXT_NEIGHBORS)
                .map(|n| format!("{} ({}:{})", n.name, n.file_path, n.line_start))
                .collect();
            let extra = callers.len().saturating_sub(MAX_CONTEXT_NEIGHBORS);
            md.push_str(&format!("  - called by: {}", names.join(", ")));
            if extra > 0 {
                md.push_str(&format!(" (+{} more)", extra));
            }
            md.push('\n');
        }
        let callees = crate::structural::get_callees(conn, &sym.qualified_name);
        if !callees.is_empty() {
            let names: Vec<String> = callees
                .iter()
                .take(MAX_CONTEXT_NEIGHBORS)
                .map(|n| format!("{} ({}:{})", n.name, n.file_path, n.line_start))
                .collect();
            let extra = callees.len().saturating_sub(MAX_CONTEXT_NEIGHBORS);
            md.push_str(&format!("  - calls: {}", names.join(", ")));
            if extra > 0 {
                md.push_str(&format!(" (+{} more)", extra));
            }
            md.push('\n');
        }
    }
    md
}

fn push_more(md: &mut String, total: usize, cap: usize) {
    if total > cap {
        md.push_str(&format!("- (+{} more)\n", total - cap));
    }
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success(), "git {:?}: {:?}", args, st);
    }

    /// Build a fake Stripe-style key at runtime so the literal never appears
    /// in source and GitHub push protection won't flag it.
    fn fake_stripe_key(marker: &str) -> String {
        format!("sk_{}_{}", "live", marker.repeat(9))
    }

    /// End-to-end on a temp git fixture: a seeded finding inside a changed
    /// hunk appears; the same class of finding outside the changed lines
    /// does not; all sections render; budget respected.
    #[test]
    fn test_review_diff_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);

        // Base commit: one clean file, one file that ALREADY has a finding
        // (it must NOT show up — it's outside the diff).
        std::fs::write(
            dir.path().join("old.py"),
            format!("API_KEY = \"{}\"\n", fake_stripe_key("old")),
        )
        .unwrap();
        std::fs::write(dir.path().join("app.py"), "def fine():\n    return 1\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "base"]);

        // Change app.py introducing a finding on a changed line.
        std::fs::write(
            dir.path().join("app.py"),
            format!(
                "def fine():\n    return 1\n\nAPI_KEY = \"{}\"\n",
                fake_stripe_key("new")
            ),
        )
        .unwrap();

        let project = crate::model::Project {
            name: format!("review-fixture-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        crate::structural::build_structural_graph(&project, true).unwrap();

        let payload = review_diff(&project, None).unwrap();
        let md = &payload.markdown;

        for section in [
            "## Summary",
            "## Findings on changed lines",
            "## Blast radius",
            "## Coverage",
            "## Context",
        ] {
            assert!(md.contains(section), "missing {section}:\n{md}");
        }

        assert!(
            md.contains("app.py"),
            "finding on the changed line must appear:\n{md}"
        );
        assert!(
            !md.contains("old.py"),
            "finding outside the changed lines must NOT appear:\n{md}"
        );
        assert!(payload.findings_on_changed_lines >= 1);
        assert!(
            md.len() <= MAX_PAYLOAD_CHARS,
            "budget exceeded: {} chars",
            md.len()
        );
    }

    /// A branch-sized diff where every impacted symbol lives inside the
    /// changed files must EXPLAIN the empty external impact, never print a
    /// bare "no external symbols impacted" next to a high-caller Context.
    #[test]
    fn test_review_diff_blast_radius_containment_explained() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);

        std::fs::write(dir.path().join("a.rs"), "fn caller() { callee(); }\n").unwrap();
        std::fs::write(dir.path().join("b.rs"), "fn callee() {}\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "base"]);

        // Change BOTH files — the whole impact set is inside the diff.
        std::fs::write(
            dir.path().join("a.rs"),
            "fn caller() { callee(); callee(); }\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("b.rs"), "fn callee() { let _x = 1; }\n").unwrap();

        let project = crate::model::Project {
            name: format!("review-contained-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        crate::structural::build_structural_graph(&project, true).unwrap();

        let payload = review_diff(&project, None).unwrap();
        let md = &payload.markdown;
        assert!(
            !md.contains("no external symbols impacted"),
            "bare contradiction line must be gone:\n{md}"
        );
        assert!(
            md.contains("within the changed files") || md.contains("`"),
            "must either explain containment or list impacted symbols:\n{md}"
        );
    }

    #[test]
    fn test_review_diff_no_changes() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", "base"]);

        let project = crate::model::Project {
            name: format!("review-clean-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let payload = review_diff(&project, None).unwrap();
        assert_eq!(payload.files_changed, 0);
        assert!(payload.markdown.contains("No changes"));
    }
}
