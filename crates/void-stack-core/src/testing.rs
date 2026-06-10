//! Test selection for diffs: which tests cover the symbols a diff touches.
//!
//! Builds a reverse coverage map from the structural graph — for every test
//! node, BFS its callees and invert into `function → [tests]` — cached in
//! the structural SQLite DB (`test_coverage_map`) and rebuilt lazily when
//! the graph changes. `suggest_tests_for_diff` then maps a git diff to
//! covering tests (closest hop first) plus an explicit *uncovered* report.

#![cfg(feature = "structural")]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rusqlite::Connection;
use serde::Serialize;

use crate::diff::{ChangedSymbol, get_changed_hunks, hunks_to_symbols};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::structural::{get_callees, nodes_by_qnames, open_db};

/// Default BFS depth when building the coverage map: a test usually reaches
/// the code it exercises within 3 calls (test → helper → api → impl).
pub const DEFAULT_COVERAGE_DEPTH: u8 = 3;

// ── Output model ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TestSuggestion {
    pub test_qualified: String,
    pub name: String,
    pub file: String,
    pub line: usize,
    /// Call-graph distance from the changed symbol (1 = direct call).
    pub hops: u8,
    /// Distinct changed symbols this test covers — the primary ranking key:
    /// on large diffs everything is hop 1 and hop-only ranking degenerates
    /// to alphabetical.
    pub covers: usize,
    pub language: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestSuggestions {
    pub suggested: Vec<TestSuggestion>,
    /// Changed non-test symbols with ZERO covering tests — first-class
    /// output: these are the blind spots of the diff.
    pub uncovered: Vec<ChangedSymbol>,
    /// Ready-to-paste runner commands, grouped per language/crate.
    pub commands: Vec<String>,
    pub changed_symbols_total: usize,
}

// ── Coverage map (cached in the structural DB) ──────────────

/// Build (or reuse) the `function → tests` coverage map. The cache key is
/// `MAX(updated_at)` over the node table: any graph rebuild that touched a
/// file changes it, which lazily invalidates the map.
pub fn ensure_coverage_map(conn: &Connection, depth: u8) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS test_coverage_map (
            function_qn TEXT NOT NULL,
            test_qn TEXT NOT NULL,
            hops INTEGER NOT NULL,
            PRIMARY KEY (function_qn, test_qn)
        );
        CREATE INDEX IF NOT EXISTS idx_tcm_function ON test_coverage_map(function_qn);",
    )
    .map_err(|e| e.to_string())?;

    let graph_stamp: f64 = conn
        .query_row("SELECT COALESCE(MAX(updated_at), 0) FROM nodes", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let stamp_key = format!("{}@{}", graph_stamp, depth);

    let cached: Option<String> = conn
        .query_row(
            "SELECT value FROM stats WHERE key = 'coverage_map_stamp'",
            [],
            |r| r.get(0),
        )
        .ok();
    if cached.as_deref() == Some(stamp_key.as_str()) {
        return Ok(());
    }

    conn.execute("DELETE FROM test_coverage_map", [])
        .map_err(|e| e.to_string())?;

    let tests: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT qualified_name FROM nodes WHERE is_test = 1 OR kind = 'Test'")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.flatten().collect()
    };

    let mut insert = conn
        .prepare(
            "INSERT INTO test_coverage_map (function_qn, test_qn, hops) VALUES (?1, ?2, ?3) \
             ON CONFLICT(function_qn, test_qn) DO UPDATE SET hops = MIN(hops, excluded.hops)",
        )
        .map_err(|e| e.to_string())?;

    for test_qn in &tests {
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(test_qn.clone());
        let mut frontier: Vec<String> = vec![test_qn.clone()];

        for hop in 1..=depth {
            let mut next: Vec<String> = Vec::new();
            for qn in &frontier {
                for callee in get_callees(conn, qn) {
                    if !visited.insert(callee.qualified_name.clone()) {
                        continue;
                    }
                    let is_test =
                        callee.is_test || matches!(callee.kind, crate::structural::NodeKind::Test);
                    if !is_test {
                        insert
                            .execute(rusqlite::params![
                                callee.qualified_name,
                                test_qn,
                                hop as i64
                            ])
                            .map_err(|e| e.to_string())?;
                    }
                    next.push(callee.qualified_name);
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
    }
    drop(insert);

    conn.execute(
        "INSERT OR REPLACE INTO stats (key, value) VALUES ('coverage_map_stamp', ?1)",
        [&stamp_key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Suggestion ──────────────────────────────────────────────

/// Suggest tests covering the current diff (against `git_base`, default
/// `HEAD`).
pub fn suggest_tests_for_diff(
    project: &Project,
    git_base: Option<&str>,
    max_results: usize,
) -> Result<TestSuggestions, String> {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    let hunks = get_changed_hunks(&root, git_base);
    let conn = open_db(project)?;
    ensure_coverage_map(&conn, DEFAULT_COVERAGE_DEPTH)?;
    let symbols = hunks_to_symbols(&conn, &hunks);
    suggest_for_symbols(&conn, &symbols, max_results)
}

/// Core logic on already-mapped symbols — unit-testable without git.
pub fn suggest_for_symbols(
    conn: &Connection,
    symbols: &[ChangedSymbol],
    max_results: usize,
) -> Result<TestSuggestions, String> {
    // test_qn → (best hop, distinct changed symbols covered).
    let mut best_hop: HashMap<String, u8> = HashMap::new();
    let mut covers: HashMap<String, HashSet<String>> = HashMap::new();
    let mut uncovered: Vec<ChangedSymbol> = Vec::new();

    for sym in symbols {
        if sym.is_test {
            continue;
        }
        // Docs/config/lockfiles can't have covering tests — reporting them
        // as "uncovered" is noise (CHANGELOG.md, Cargo.toml, …).
        if sym.kind == "file" && !is_source_file(&sym.file) {
            continue;
        }
        let covering = if sym.kind == "file" {
            // File-level change: any test covering any symbol of the file.
            tests_for_prefix(conn, &format!("{}::%", sym.file))?
        } else {
            tests_for_function(conn, &sym.qualified_name)?
        };

        if covering.is_empty() {
            uncovered.push(sym.clone());
            continue;
        }
        for (test_qn, hops) in covering {
            covers
                .entry(test_qn.clone())
                .or_default()
                .insert(sym.qualified_name.clone());
            best_hop
                .entry(test_qn)
                .and_modify(|h| *h = (*h).min(hops))
                .or_insert(hops);
        }
    }

    // Hydrate test nodes for names/locations.
    let qns: Vec<String> = best_hop.keys().cloned().collect();
    let nodes = nodes_by_qnames(conn, &qns)?;
    let mut suggested: Vec<TestSuggestion> = nodes
        .into_iter()
        .map(|n| TestSuggestion {
            hops: best_hop.get(&n.qualified_name).copied().unwrap_or(0),
            covers: covers.get(&n.qualified_name).map(|s| s.len()).unwrap_or(0),
            test_qualified: n.qualified_name,
            name: n.name,
            file: n.file_path,
            line: n.line_start,
            language: n.language,
        })
        .collect();
    // Coverage density first (a broad integration test outranks a narrow
    // unit test), then proximity, then name for determinism.
    suggested.sort_by(|a, b| {
        b.covers
            .cmp(&a.covers)
            .then_with(|| a.hops.cmp(&b.hops))
            .then_with(|| a.name.cmp(&b.name))
    });
    suggested.truncate(max_results);

    let commands = runner_commands(&suggested);
    Ok(TestSuggestions {
        suggested,
        uncovered,
        commands,
        changed_symbols_total: symbols.len(),
    })
}

fn tests_for_function(conn: &Connection, qn: &str) -> Result<Vec<(String, u8)>, String> {
    let mut stmt = conn
        .prepare("SELECT test_qn, hops FROM test_coverage_map WHERE function_qn = ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([qn], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u8))
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

fn tests_for_prefix(conn: &Connection, like: &str) -> Result<Vec<(String, u8)>, String> {
    let mut stmt = conn
        .prepare("SELECT test_qn, MIN(hops) FROM test_coverage_map WHERE function_qn LIKE ?1 GROUP BY test_qn")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([like], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u8))
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

// ── Runner commands ─────────────────────────────────────────

/// Ready-to-paste commands per language. Rust commands are grouped per
/// crate (workspace layout `crates/<name>/…`); JS/TS are best-effort and
/// labeled as suggested.
fn runner_commands(suggested: &[TestSuggestion]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for t in suggested {
        let cmd = match t.language.as_str() {
            "rust" => match rust_crate_for(&t.file) {
                Some(krate) => format!("cargo test -p {} {}", krate, t.name),
                None => format!("cargo test {}", t.name),
            },
            "go" => {
                let pkg = t.file.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
                format!("go test ./{} -run '^{}$'", pkg, t.name)
            }
            "dart" => format!("flutter test {}", t.file),
            "javascript" | "typescript" => {
                format!("npx jest {} -t '{}'  # suggested", t.file, t.name)
            }
            _ => continue,
        };
        if seen.insert(cmd.clone()) {
            out.push(cmd);
        }
    }
    out
}

/// True for files that can plausibly have covering tests.
fn is_source_file(file: &str) -> bool {
    matches!(
        file.rsplit('.')
            .next()
            .unwrap_or("")
            .to_lowercase()
            .as_str(),
        "rs" | "go"
            | "dart"
            | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "java"
            | "kt"
            | "rb"
            | "php"
            | "c"
            | "cpp"
            | "h"
            | "cs"
            | "ex"
            | "exs"
            | "swift"
            | "vue"
            | "svelte"
    )
}

fn rust_crate_for(file: &str) -> Option<String> {
    let norm = file.replace('\\', "/");
    let rest = norm.strip_prefix("crates/")?;
    let krate = rest.split('/').next()?;
    if krate.is_empty() {
        None
    } else {
        Some(krate.to_string())
    }
}

// ── Markdown rendering (shared by MCP + CLI) ────────────────

/// Compact markdown for the suggestion set.
pub fn render_suggestions_markdown(s: &TestSuggestions) -> String {
    let mut md = String::new();
    md.push_str(&format!(
        "## Suggested tests ({} for {} changed symbols)\n",
        s.suggested.len(),
        s.changed_symbols_total
    ));
    if s.suggested.is_empty() {
        md.push_str("- none found via the structural coverage map\n");
    }
    for t in &s.suggested {
        md.push_str(&format!(
            "- `{}` — {}:{} — covers {} changed symbol{} (hop {})\n",
            t.name,
            t.file,
            t.line,
            t.covers,
            if t.covers == 1 { "" } else { "s" },
            t.hops
        ));
    }

    md.push_str(&format!("\n## Uncovered ({})\n", s.uncovered.len()));
    if s.uncovered.is_empty() {
        md.push_str("- every changed symbol has at least one covering test\n");
    }
    const MAX_UNCOVERED_LISTED: usize = 15;
    for u in s.uncovered.iter().take(MAX_UNCOVERED_LISTED) {
        let tag = if u.is_new_file { " (new file)" } else { "" };
        md.push_str(&format!(
            "- ⚠️ `{}` — {}:{}{} has NO covering tests\n",
            u.name, u.file, u.line_start, tag
        ));
    }
    if s.uncovered.len() > MAX_UNCOVERED_LISTED {
        md.push_str(&format!(
            "- (+{} more)\n",
            s.uncovered.len() - MAX_UNCOVERED_LISTED
        ));
    }

    if !s.commands.is_empty() {
        md.push_str("\n## Run\n```sh\n");
        for c in &s.commands {
            md.push_str(c);
            md.push('\n');
        }
        md.push_str("```\n");
    }
    md
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::{EdgeKind, NodeKind, StructuralEdge, StructuralNode, store_file};

    fn fixture_project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: format!("testing-fixture-{}-{:p}", std::process::id(), &dir),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        (dir, project)
    }

    fn node(file: &str, name: &str, ls: usize, test: bool) -> StructuralNode {
        StructuralNode {
            kind: if test {
                NodeKind::Test
            } else {
                NodeKind::Function
            },
            name: name.to_string(),
            qualified_name: format!("{}::{}", file, name),
            file_path: file.to_string(),
            line_start: ls,
            line_end: ls + 5,
            language: "rust".to_string(),
            parent_name: None,
            is_test: test,
        }
    }

    fn call(src: &str, dst: &str, file: &str) -> StructuralEdge {
        StructuralEdge {
            kind: EdgeKind::Calls,
            source_qualified: src.to_string(),
            target_qualified: dst.to_string(),
            file_path: file.to_string(),
            line: 1,
        }
    }

    fn changed(file: &str, name: &str, ls: usize) -> ChangedSymbol {
        ChangedSymbol {
            qualified_name: format!("{}::{}", file, name),
            name: name.to_string(),
            kind: "Function".to_string(),
            language: "rust".to_string(),
            file: file.to_string(),
            line_start: ls,
            line_end: ls + 5,
            is_test: false,
            is_new_file: false,
        }
    }

    /// test_t → A → B: a diff touching B must suggest test_t at hop 2;
    /// touching uncovered C must report it as uncovered.
    #[test]
    fn test_suggestion_via_call_chain_and_uncovered() {
        let (_dir, project) = fixture_project();
        let conn = open_db(&project).unwrap();

        store_file(
            &conn,
            "crates/core/tests/t.rs",
            &[node("crates/core/tests/t.rs", "test_t", 1, true)],
            &[call(
                "crates/core/tests/t.rs::test_t",
                "crates/core/src/a.rs::A",
                "crates/core/tests/t.rs",
            )],
            "h1",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/core/src/a.rs",
            &[node("crates/core/src/a.rs", "A", 1, false)],
            &[call(
                "crates/core/src/a.rs::A",
                "crates/core/src/b.rs::B",
                "crates/core/src/a.rs",
            )],
            "h2",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/core/src/b.rs",
            &[
                node("crates/core/src/b.rs", "B", 1, false),
                node("crates/core/src/b.rs", "C", 20, false),
            ],
            &[],
            "h3",
        )
        .unwrap();

        ensure_coverage_map(&conn, 3).unwrap();

        let result =
            suggest_for_symbols(&conn, &[changed("crates/core/src/b.rs", "B", 1)], 10).unwrap();
        assert_eq!(result.suggested.len(), 1, "{:?}", result.suggested);
        assert_eq!(result.suggested[0].name, "test_t");
        assert_eq!(result.suggested[0].hops, 2);
        assert!(result.uncovered.is_empty());
        assert_eq!(
            result.commands,
            vec!["cargo test -p core test_t".to_string()]
        );

        // C has no covering tests — must be a first-class uncovered entry.
        let result =
            suggest_for_symbols(&conn, &[changed("crates/core/src/b.rs", "C", 20)], 10).unwrap();
        assert!(result.suggested.is_empty());
        assert_eq!(result.uncovered.len(), 1);
        assert_eq!(result.uncovered[0].name, "C");
    }

    /// Rebuilding the graph (new updated_at stamp) must invalidate the
    /// cached coverage map: a newly added test shows up without manual
    /// cache clearing.
    #[test]
    fn test_coverage_map_invalidated_on_graph_rebuild() {
        let (_dir, project) = fixture_project();
        let conn = open_db(&project).unwrap();

        store_file(
            &conn,
            "src/b.rs",
            &[node("src/b.rs", "B", 1, false)],
            &[],
            "h1",
        )
        .unwrap();
        ensure_coverage_map(&conn, 3).unwrap();
        let r = suggest_for_symbols(&conn, &[changed("src/b.rs", "B", 1)], 10).unwrap();
        assert!(r.suggested.is_empty(), "no tests yet");

        // Graph rebuild adds a covering test (updated_at advances).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        store_file(
            &conn,
            "tests/new.rs",
            &[node("tests/new.rs", "test_new", 1, true)],
            &[call(
                "tests/new.rs::test_new",
                "src/b.rs::B",
                "tests/new.rs",
            )],
            "h2",
        )
        .unwrap();

        ensure_coverage_map(&conn, 3).unwrap();
        let r = suggest_for_symbols(&conn, &[changed("src/b.rs", "B", 1)], 10).unwrap();
        assert_eq!(
            r.suggested.len(),
            1,
            "stale map must rebuild after graph change"
        );
        assert_eq!(r.suggested[0].name, "test_new");
    }

    /// A broad integration test covering 3 changed symbols must outrank a
    /// narrow unit test at the same hop distance.
    #[test]
    fn test_ranking_by_coverage_density() {
        let (_dir, project) = fixture_project();
        let conn = open_db(&project).unwrap();

        // broad_test → A, B, C; narrow_test → A.
        store_file(
            &conn,
            "crates/core/tests/broad.rs",
            &[node("crates/core/tests/broad.rs", "test_broad", 1, true)],
            &[
                call(
                    "crates/core/tests/broad.rs::test_broad",
                    "crates/core/src/l.rs::A",
                    "crates/core/tests/broad.rs",
                ),
                call(
                    "crates/core/tests/broad.rs::test_broad",
                    "crates/core/src/l.rs::B",
                    "crates/core/tests/broad.rs",
                ),
                call(
                    "crates/core/tests/broad.rs::test_broad",
                    "crates/core/src/l.rs::C",
                    "crates/core/tests/broad.rs",
                ),
            ],
            "h1",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/core/tests/narrow.rs",
            &[node("crates/core/tests/narrow.rs", "test_a_only", 1, true)],
            &[call(
                "crates/core/tests/narrow.rs::test_a_only",
                "crates/core/src/l.rs::A",
                "crates/core/tests/narrow.rs",
            )],
            "h2",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/core/src/l.rs",
            &[
                node("crates/core/src/l.rs", "A", 1, false),
                node("crates/core/src/l.rs", "B", 10, false),
                node("crates/core/src/l.rs", "C", 20, false),
            ],
            &[],
            "h3",
        )
        .unwrap();
        ensure_coverage_map(&conn, 3).unwrap();

        let result = suggest_for_symbols(
            &conn,
            &[
                changed("crates/core/src/l.rs", "A", 1),
                changed("crates/core/src/l.rs", "B", 10),
                changed("crates/core/src/l.rs", "C", 20),
            ],
            10,
        )
        .unwrap();
        assert_eq!(
            result.suggested[0].name, "test_broad",
            "{:?}",
            result.suggested
        );
        assert_eq!(result.suggested[0].covers, 3);
        assert_eq!(result.suggested[1].name, "test_a_only");
        assert_eq!(result.suggested[1].covers, 1);

        let md = render_suggestions_markdown(&result);
        assert!(md.contains("covers 3 changed symbols (hop 1)"), "{md}");
    }

    #[test]
    fn test_runner_commands_per_language() {
        let mk = |lang: &str, file: &str, name: &str| TestSuggestion {
            test_qualified: format!("{}::{}", file, name),
            name: name.to_string(),
            file: file.to_string(),
            line: 1,
            hops: 1,
            covers: 1,
            language: lang.to_string(),
        };
        let cmds = runner_commands(&[
            mk("rust", "crates/void-stack-core/src/x.rs", "test_x"),
            mk("go", "internal/api/handler_test.go", "TestHandler"),
            mk("dart", "test/auth_test.dart", "login works"),
            mk("typescript", "src/api.test.ts", "creates order"),
        ]);
        assert!(cmds.contains(&"cargo test -p void-stack-core test_x".to_string()));
        assert!(cmds.contains(&"go test ./internal/api -run '^TestHandler$'".to_string()));
        assert!(cmds.contains(&"flutter test test/auth_test.dart".to_string()));
        assert!(
            cmds.iter()
                .any(|c| c.starts_with("npx jest src/api.test.ts") && c.contains("# suggested"))
        );
    }
}
