//! Query helpers over the structural graph.
//!
//! Impact-radius uses a recursive CTE in SQLite that walks both directions
//! (what the seed calls + what calls the seed) up to `max_depth` and caps
//! the result at `max_nodes`.

// Bidirectional BFS impact radius via SQLite recursive CTE.
// Query logic adapted from code-review-graph by Tirth Patel (tirth8205)
// https://github.com/tirth8205/code-review-graph — MIT License

use serde::Serialize;

use rusqlite::Connection;

use super::graph::{nodes_by_qnames, qnames_in_files};
use super::parser::{NodeKind, StructuralNode};

#[derive(Debug, Clone, Serialize)]
pub struct ImpactResult {
    pub changed_files: Vec<String>,
    pub impacted_nodes: Vec<StructuralNode>,
    pub impacted_files: Vec<String>,
    pub max_depth: usize,
    pub truncated: bool,
}

/// Bidirectional BFS starting from every node in `changed_files` using a
/// recursive SQLite CTE. Returns distinct impacted nodes (callers ∪ callees
/// up to `max_depth`) plus the set of files they touch.
///
/// `only_calls = true` restricts traversal to `CALLS` edges, which avoids
/// the IMPORTS_FROM edge blow-up that makes TypeScript/JavaScript graphs
/// fan out into the thousands of neighbours per node.
pub fn get_impact_radius(
    conn: &Connection,
    changed_files: &[String],
    max_depth: usize,
    max_nodes: usize,
    only_calls: bool,
) -> Result<ImpactResult, String> {
    let seeds = qnames_in_files(conn, changed_files)?;
    if seeds.is_empty() {
        return Ok(ImpactResult {
            changed_files: changed_files.to_vec(),
            impacted_nodes: Vec::new(),
            impacted_files: Vec::new(),
            max_depth,
            truncated: false,
        });
    }

    conn.execute_batch(
        "DROP TABLE IF EXISTS _impact_seeds;
         CREATE TEMP TABLE _impact_seeds(qn TEXT PRIMARY KEY);",
    )
    .map_err(|e| e.to_string())?;
    {
        let mut ins = conn
            .prepare("INSERT OR IGNORE INTO _impact_seeds(qn) VALUES (?1)")
            .map_err(|e| e.to_string())?;
        for qn in &seeds {
            ins.execute([qn]).map_err(|e| e.to_string())?;
        }
    }

    // Bidirectional BFS CTE: forward along source→target and back along
    // target→source. `max_depth` caps recursion; `max_nodes` caps result size.
    // When `only_calls` is set the ?3 flag is 1 and the JOINs drop every
    // non-CALLS edge — essential for TS/JS graphs where IMPORTS_FROM edges
    // dominate (~29/node) and otherwise blow up the working set.
    let sql = "WITH RECURSIVE impacted(node_qn, depth) AS (
        SELECT qn, 0 FROM _impact_seeds
        UNION
        SELECT e.target_qualified, i.depth + 1
        FROM impacted i
        JOIN edges e ON e.source_qualified = i.node_qn
          AND (?3 = 0 OR e.kind = 'CALLS')
        WHERE i.depth < ?1
        UNION
        SELECT e.source_qualified, i.depth + 1
        FROM impacted i
        JOIN edges e ON e.target_qualified = i.node_qn
          AND (?3 = 0 OR e.kind = 'CALLS')
        WHERE i.depth < ?1
    )
    SELECT node_qn FROM (
        SELECT node_qn, MIN(depth) AS min_depth
        FROM impacted
        GROUP BY node_qn
    )
    ORDER BY min_depth, node_qn
    LIMIT ?2";

    let only_calls_flag: i64 = if only_calls { 1 } else { 0 };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![max_depth as i64, max_nodes as i64, only_calls_flag],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?;
    let impacted_qns: Vec<String> = rows.flatten().collect();

    let truncated = impacted_qns.len() >= max_nodes;
    let impacted_nodes = nodes_by_qnames(conn, &impacted_qns)?;
    let mut files: Vec<String> = impacted_nodes.iter().map(|n| n.file_path.clone()).collect();
    files.sort();
    files.dedup();

    Ok(ImpactResult {
        changed_files: changed_files.to_vec(),
        impacted_nodes,
        impacted_files: files,
        max_depth,
        truncated,
    })
}

/// Last `::` segment of a qualified name (`a.dart::Cls::m` → `m`).
fn last_segment(qualified_name: &str) -> &str {
    qualified_name.rsplit("::").next().unwrap_or(qualified_name)
}

/// `file::Cls::m` → `Cls::m` (typed-receiver form), None for 1-segment qns.
fn last_two_segments(qualified_name: &str) -> Option<String> {
    let mut it = qualified_name.rsplit("::");
    let tail = it.next()?;
    let parent = it.next()?;
    if parent.contains('/') || parent.contains('.') {
        return None; // parent is the file, not a type
    }
    Some(format!("{}::{}", parent, tail))
}

/// Names so common that an unresolved bare call edge says nothing about
/// WHICH definition it targets. Such edges are low-confidence and excluded
/// from caller counts, Context sections and GraphRAG expansion by default.
const ULTRA_COMMON_NAMES: &[&str] = &[
    "new",
    "from",
    "default",
    "get",
    "set",
    "path",
    "fmt",
    "clone",
    "len",
    "build",
    "init",
    "main",
    "run",
    "next",
    "into",
    "parse",
    "write",
    "read",
    "map",
    "insert",
    "push",
    "create",
    "update",
    "delete",
    "value",
    "name",
    "id",
    "to_string",
    "as_str",
    "close",
    "open",
    "start",
    "stop",
    "add",
    "remove",
    "send",
    "call",
    "apply",
    "execute",
];

/// Crate-ish prefix used for the same-module disambiguation heuristic:
/// `crates/<name>/` for cargo workspaces, otherwise the first path segment.
fn module_prefix(file_path: &str) -> String {
    let norm = file_path.replace('\\', "/");
    let parts: Vec<&str> = norm.split('/').collect();
    if parts.first() == Some(&"crates") && parts.len() > 1 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        parts.first().unwrap_or(&"").to_string()
    }
}

/// All definition sites (file paths) for a bare name.
fn definition_files(conn: &Connection, name: &str) -> Vec<String> {
    let mut stmt = match conn
        .prepare("SELECT DISTINCT file_path FROM nodes WHERE name = ?1 AND kind != 'File'")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    match stmt.query_map([name], |r| r.get::<_, String>(0)) {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

/// True when `caller_file` has an IMPORTS_FROM edge mentioning the stem of
/// the defining file (`auth_service` for `lib/auth_service.dart`).
fn imports_defining_file(conn: &Connection, caller_file: &str, defining_file: &str) -> bool {
    let stem = defining_file
        .rsplit('/')
        .next()
        .unwrap_or(defining_file)
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(defining_file);
    if stem.is_empty() {
        return false;
    }
    conn.query_row(
        "SELECT 1 FROM edges WHERE kind = 'IMPORTS_FROM' AND file_path = ?1          AND target_qualified LIKE ?2 LIMIT 1",
        rusqlite::params![caller_file, format!("%{}%", stem)],
        |_| Ok(()),
    )
    .is_ok()
}

/// Functions that call `qualified_name` (reverse edges).
///
/// Matches both fully-qualified edge targets and *bare-name* targets:
/// `resolve_call_targets` only qualifies same-file calls, so cross-file
/// call edges keep the callee's bare name (`loginWithGoogle`). Without the
/// bare match every cross-file caller was invisible — on layered codebases
/// (Flutter widgets → services) that meant 0 callers for everything.
pub fn get_callers(conn: &Connection, qualified_name: &str) -> Vec<StructuralNode> {
    get_callers_opt(conn, qualified_name, false)
}

/// Like [`get_callers`], with control over low-confidence bare-name edges.
///
/// Bare cross-file call edges are disambiguated in order: typed-receiver
/// hint (`Foo::new` edges only match `Foo`'s `new`), same file, same
/// module/crate with a single definition there, import graph (caller file
/// imports the defining file). What remains — including every ultra-common
/// name with multiple definitions — is LOW CONFIDENCE: two different `new`
/// symbols used to report the same ~664 callers because every `.new(` edge
/// matched both. Low-confidence callers are excluded unless
/// `include_low_confidence` is set.
pub fn get_callers_opt(
    conn: &Connection,
    qualified_name: &str,
    include_low_confidence: bool,
) -> Vec<StructuralNode> {
    let bare = last_segment(qualified_name);
    let typed = last_two_segments(qualified_name);

    let mut qns: Vec<String> = Vec::new();

    // 1. Exact qualified edges + typed-receiver edges: high confidence.
    {
        let typed_param = typed.clone().unwrap_or_else(|| qualified_name.to_string());
        let mut stmt = match conn.prepare(
            "SELECT DISTINCT source_qualified FROM edges \
             WHERE (target_qualified = ?1 OR target_qualified = ?2) AND kind = 'CALLS'",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        if let Ok(rows) = stmt.query_map([qualified_name, typed_param.as_str()], |r| {
            r.get::<_, String>(0)
        }) {
            qns.extend(rows.flatten());
        }
    }

    // 2. Bare edges, disambiguated.
    let defining_file = qualified_name.split("::").next().unwrap_or("");
    let defs = definition_files(conn, bare);
    let ambiguous = defs.len() > 1;
    let my_prefix = module_prefix(defining_file);
    let defs_in_my_module = defs
        .iter()
        .filter(|f| module_prefix(f) == my_prefix)
        .count();

    let bare_edges: Vec<(String, String)> = {
        let mut stmt = match conn.prepare(
            "SELECT DISTINCT source_qualified, file_path FROM edges \
             WHERE target_qualified = ?1 AND kind = 'CALLS'",
        ) {
            Ok(s) => s,
            Err(_) => return nodes_by_qnames(conn, &qns).unwrap_or_default(),
        };
        match stmt.query_map([bare], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        }
    };

    for (source, caller_file) in bare_edges {
        let same_module_unique = module_prefix(&caller_file) == my_prefix && defs_in_my_module == 1;
        // Ultra-common names trust NOTHING but the same file: even a
        // unique-in-graph `path` is shadowed by stdlib methods
        // (`dir.path()`), so "single definition" proves nothing. Real
        // cross-file calls still resolve via typed-receiver edges
        // (`Foo::new`). Everything else: unique definition, same file,
        // unique-in-module, then the import graph.
        let high = if ULTRA_COMMON_NAMES.contains(&bare) {
            caller_file == defining_file
        } else if !ambiguous || caller_file == defining_file || same_module_unique {
            true
        } else {
            imports_defining_file(conn, &caller_file, defining_file)
        };
        if high || include_low_confidence {
            qns.push(source);
        }
    }

    qns.sort();
    qns.dedup();
    nodes_by_qnames(conn, &qns).unwrap_or_default()
}

/// Functions called by `qualified_name` (forward edges).
///
/// Edge targets that are still bare names (unresolved cross-file calls)
/// are resolved here against the node table by `name`, capped so common
/// names (`build`, `init`) can't explode the result.
pub fn get_callees(conn: &Connection, qualified_name: &str) -> Vec<StructuralNode> {
    get_callees_opt(conn, qualified_name, false)
}

/// Like [`get_callees`], with control over low-confidence bare-name edges.
/// Unresolved bare targets resolve to: the single definition when only one
/// exists; the same-file definition; the single same-module definition;
/// otherwise they are low-confidence and dropped unless requested.
pub fn get_callees_opt(
    conn: &Connection,
    qualified_name: &str,
    include_low_confidence: bool,
) -> Vec<StructuralNode> {
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT target_qualified FROM edges \
         WHERE source_qualified = ?1 AND kind = 'CALLS'",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let targets: Vec<String> = match stmt.query_map([qualified_name], |r| r.get::<_, String>(0)) {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => return Vec::new(),
    };

    let caller_file = qualified_name.split("::").next().unwrap_or("");
    let caller_prefix = module_prefix(caller_file);

    let mut resolved: Vec<String> = Vec::new();
    let mut low: Vec<String> = Vec::new();

    for target in targets {
        if target.contains('/') || (target.contains("::") && target.contains('.')) {
            // Fully qualified (file::…).
            resolved.push(target);
            continue;
        }
        if let Some((recv, tail)) = target.split_once("::") {
            // Typed-receiver hint: nodes with parent_name = recv, name = tail.
            resolved.extend(nodes_with_parent(conn, recv, tail));
            continue;
        }
        // Bare name. Ultra-common names resolve same-file only (stdlib
        // methods shadow even unique-in-graph definitions).
        let defs = definition_files(conn, &target);
        let pick = if defs.iter().any(|f| f == caller_file) {
            Some(caller_file.to_string())
        } else if ULTRA_COMMON_NAMES.contains(&target.as_str()) {
            None
        } else if defs.len() == 1 {
            Some(defs[0].clone())
        } else {
            let in_module: Vec<&String> = defs
                .iter()
                .filter(|f| module_prefix(f) == caller_prefix)
                .collect();
            if in_module.len() == 1 {
                Some(in_module[0].clone())
            } else {
                None
            }
        };
        match pick {
            Some(file) => resolved.extend(qns_for_name_in_file(conn, &target, &file)),
            None => low.push(target),
        }
    }

    let mut nodes = nodes_by_qnames(conn, &resolved).unwrap_or_default();
    if include_low_confidence && !low.is_empty() {
        nodes.extend(super::graph::nodes_by_names(conn, &low, 20).unwrap_or_default());
    }
    nodes
}

fn nodes_with_parent(conn: &Connection, parent: &str, name: &str) -> Vec<String> {
    let mut stmt = match conn
        .prepare("SELECT qualified_name FROM nodes WHERE parent_name = ?1 AND name = ?2")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    match stmt.query_map([parent, name], |r| r.get::<_, String>(0)) {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

fn qns_for_name_in_file(conn: &Connection, name: &str, file: &str) -> Vec<String> {
    let mut stmt =
        match conn.prepare("SELECT qualified_name FROM nodes WHERE name = ?1 AND file_path = ?2") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
    match stmt.query_map([name, file], |r| r.get::<_, String>(0)) {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

/// Tests that (directly or transitively at depth 1) call `qualified_name`.
pub fn get_tests_for(conn: &Connection, qualified_name: &str) -> Vec<StructuralNode> {
    let callers = get_callers(conn, qualified_name);
    callers
        .into_iter()
        .filter(|n| matches!(n.kind, NodeKind::Test) || n.is_test)
        .collect()
}

/// Fuzzy search nodes by substring match on name or qualified_name.
///
/// Exact name matches rank first, then qualified names ending in
/// `::<query>`, then substring hits — without the ordering, a LIKE over a
/// 45k-node graph returned an arbitrary first page and the right node was
/// often not among the `limit` rows at all.
pub fn search_nodes(conn: &Connection, query: &str, limit: usize) -> Vec<StructuralNode> {
    let like = format!("%{}%", query);
    let suffix = format!("%::{}", query);
    let mut stmt = match conn.prepare(
        "SELECT kind, name, qualified_name, file_path, line_start, line_end, \
         language, parent_name, is_test FROM nodes \
         WHERE name = ?1 OR name LIKE ?2 OR qualified_name LIKE ?2 \
         ORDER BY CASE \
             WHEN name = ?1 THEN 0 \
             WHEN qualified_name LIKE ?3 THEN 1 \
             ELSE 2 END, \
             LENGTH(qualified_name) \
         LIMIT ?4",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(
        rusqlite::params![query, like, suffix, limit as i64],
        |row| {
            let kind_str: String = row.get(0)?;
            let kind = NodeKind::parse(&kind_str).unwrap_or(NodeKind::Function);
            Ok(StructuralNode {
                kind,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                file_path: row.get(3)?,
                line_start: row.get::<_, i64>(4)? as usize,
                line_end: row.get::<_, i64>(5)? as usize,
                language: row.get(6)?,
                parent_name: row.get(7)?,
                is_test: row.get::<_, i64>(8)? != 0,
            })
        },
    ) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::super::graph::{open_db, store_file};
    use super::super::parser::{EdgeKind, StructuralEdge};
    use super::*;
    use crate::model::Project;

    fn project_in(dir: &std::path::Path) -> Project {
        Project {
            name: "q-test".to_string(),
            path: dir.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    fn func(qn: &str, name: &str, file: &str) -> StructuralNode {
        StructuralNode {
            kind: NodeKind::Function,
            name: name.to_string(),
            qualified_name: qn.to_string(),
            file_path: file.to_string(),
            line_start: 1,
            line_end: 1,
            language: "rust".to_string(),
            parent_name: None,
            is_test: false,
        }
    }

    fn call(from: &str, to: &str, file: &str) -> StructuralEdge {
        StructuralEdge {
            kind: EdgeKind::Calls,
            source_qualified: from.to_string(),
            target_qualified: to.to_string(),
            file_path: file.to_string(),
            line: 1,
        }
    }

    #[test]
    fn test_get_callers_and_callees() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(tmp.path())).unwrap();
        store_file(
            &conn,
            "a.rs",
            &[func("a.rs::A", "A", "a.rs"), func("a.rs::B", "B", "a.rs")],
            &[call("a.rs::A", "a.rs::B", "a.rs")],
            "h",
        )
        .unwrap();

        let callers = get_callers(&conn, "a.rs::B");
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].qualified_name, "a.rs::A");

        let callees = get_callees(&conn, "a.rs::A");
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].qualified_name, "a.rs::B");
    }

    #[test]
    fn test_impact_radius_direct() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(tmp.path())).unwrap();
        // A calls B and C, all in same file.
        store_file(
            &conn,
            "a.rs",
            &[
                func("a.rs::A", "A", "a.rs"),
                func("a.rs::B", "B", "a.rs"),
                func("a.rs::C", "C", "a.rs"),
            ],
            &[
                call("a.rs::A", "a.rs::B", "a.rs"),
                call("a.rs::A", "a.rs::C", "a.rs"),
            ],
            "h",
        )
        .unwrap();
        let res = get_impact_radius(&conn, &["a.rs".to_string()], 2, 1000, false).unwrap();
        let qns: Vec<&str> = res
            .impacted_nodes
            .iter()
            .map(|n| n.qualified_name.as_str())
            .collect();
        assert!(qns.contains(&"a.rs::A"));
        assert!(qns.contains(&"a.rs::B"));
        assert!(qns.contains(&"a.rs::C"));
    }

    #[test]
    fn test_impact_radius_transitive() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(tmp.path())).unwrap();
        // A → B → C across two files.
        store_file(
            &conn,
            "a.rs",
            &[func("a.rs::A", "A", "a.rs")],
            &[call("a.rs::A", "b.rs::B", "a.rs")],
            "h1",
        )
        .unwrap();
        store_file(
            &conn,
            "b.rs",
            &[func("b.rs::B", "B", "b.rs"), func("b.rs::C", "C", "b.rs")],
            &[call("b.rs::B", "b.rs::C", "b.rs")],
            "h2",
        )
        .unwrap();

        let res = get_impact_radius(&conn, &["a.rs".to_string()], 2, 1000, false).unwrap();
        let qns: Vec<&str> = res
            .impacted_nodes
            .iter()
            .map(|n| n.qualified_name.as_str())
            .collect();
        assert!(qns.contains(&"a.rs::A"));
        assert!(qns.contains(&"b.rs::B"));
        assert!(
            qns.contains(&"b.rs::C"),
            "depth=2 should reach C from A, got {:?}",
            qns
        );
    }

    #[test]
    fn test_search_nodes_substring() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(tmp.path())).unwrap();
        store_file(
            &conn,
            "h.rs",
            &[
                func("h.rs::handleRequest", "handleRequest", "h.rs"),
                func("h.rs::parseJson", "parseJson", "h.rs"),
            ],
            &[],
            "h",
        )
        .unwrap();
        let found = search_nodes(&conn, "handle", 10);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "handleRequest");
    }

    #[test]
    fn test_impact_only_calls_excludes_imports() {
        use super::super::parser::EdgeKind;
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(tmp.path())).unwrap();
        // a.rs::A CALLS b.rs::B and IMPORTS_FROM c.rs::C (cross-file so
        // B/C don't land in the seed set automatically).
        store_file(
            &conn,
            "a.rs",
            &[func("a.rs::A", "A", "a.rs")],
            &[
                call("a.rs::A", "b.rs::B", "a.rs"),
                StructuralEdge {
                    kind: EdgeKind::ImportsFrom,
                    source_qualified: "a.rs::A".to_string(),
                    target_qualified: "c.rs::C".to_string(),
                    file_path: "a.rs".to_string(),
                    line: 1,
                },
            ],
            "h1",
        )
        .unwrap();
        store_file(&conn, "b.rs", &[func("b.rs::B", "B", "b.rs")], &[], "h2").unwrap();
        store_file(&conn, "c.rs", &[func("c.rs::C", "C", "c.rs")], &[], "h3").unwrap();

        let only = get_impact_radius(&conn, &["a.rs".to_string()], 2, 1000, true).unwrap();
        let qns: Vec<&str> = only
            .impacted_nodes
            .iter()
            .map(|n| n.qualified_name.as_str())
            .collect();
        assert!(qns.contains(&"a.rs::A") && qns.contains(&"b.rs::B"));
        assert!(
            !qns.contains(&"c.rs::C"),
            "only_calls=true must drop IMPORTS_FROM edges, got {:?}",
            qns
        );

        let all = get_impact_radius(&conn, &["a.rs".to_string()], 2, 1000, false).unwrap();
        let qns_all: Vec<&str> = all
            .impacted_nodes
            .iter()
            .map(|n| n.qualified_name.as_str())
            .collect();
        assert!(qns_all.contains(&"c.rs::C"));
    }

    #[test]
    fn test_impact_radius_completes_quickly_large_graph() {
        use super::super::parser::EdgeKind;
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(tmp.path())).unwrap();

        // 1000 nodes + ~20k IMPORTS_FROM edges (fan-out 20 per node).
        let mut nodes: Vec<StructuralNode> = Vec::with_capacity(1000);
        for i in 0..1000 {
            nodes.push(func(&format!("a.rs::f{}", i), &format!("f{}", i), "a.rs"));
        }
        let mut edges: Vec<StructuralEdge> = Vec::with_capacity(20_000);
        for i in 0..1000 {
            for j in 0..20 {
                let target = (i + j + 1) % 1000;
                edges.push(StructuralEdge {
                    kind: EdgeKind::ImportsFrom,
                    source_qualified: format!("a.rs::f{}", i),
                    target_qualified: format!("a.rs::f{}", target),
                    file_path: "a.rs".to_string(),
                    line: 1,
                });
            }
        }
        // Seed one CALLS edge so only_calls=true still has a path to explore.
        edges.push(call("a.rs::f0", "a.rs::f1", "a.rs"));
        store_file(&conn, "a.rs", &nodes, &edges, "h").unwrap();

        let start = std::time::Instant::now();
        let res = get_impact_radius(&conn, &["a.rs".to_string()], 2, 500, true).unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "only_calls path took {:?} on a 20k-edge IMPORTS_FROM graph",
            elapsed
        );
        assert!(!res.impacted_nodes.is_empty());
    }
}

#[cfg(test)]
mod confidence_tests {
    use super::*;
    use crate::structural::{
        EdgeKind, NodeKind, StructuralEdge, StructuralNode, open_db, store_file,
    };

    fn project_in(dir: &std::path::Path) -> crate::model::Project {
        crate::model::Project {
            name: format!("conf-{}-{:p}", std::process::id(), dir),
            path: dir.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    fn node(file: &str, parent: Option<&str>, name: &str) -> StructuralNode {
        StructuralNode {
            kind: NodeKind::Function,
            name: name.to_string(),
            qualified_name: match parent {
                Some(p) => format!("{}::{}::{}", file, p, name),
                None => format!("{}::{}", file, name),
            },
            file_path: file.to_string(),
            line_start: 1,
            line_end: 5,
            language: "rust".to_string(),
            parent_name: parent.map(|s| s.to_string()),
            is_test: false,
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

    /// Two `new` in different crates with distinct callers must resolve
    /// separately — previously both reported the union of every `.new(`.
    #[test]
    fn test_two_definitions_resolve_separately() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(dir.path())).unwrap();

        store_file(
            &conn,
            "crates/a/src/x.rs",
            &[node("crates/a/src/x.rs", Some("Foo"), "connect")],
            &[],
            "h1",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/b/src/y.rs",
            &[node("crates/b/src/y.rs", Some("Bar"), "connect")],
            &[],
            "h2",
        )
        .unwrap();
        // Caller in crate a calls bare `connect`; caller in crate b likewise.
        store_file(
            &conn,
            "crates/a/src/main.rs",
            &[node("crates/a/src/main.rs", None, "ca")],
            &[call(
                "crates/a/src/main.rs::ca",
                "connect",
                "crates/a/src/main.rs",
            )],
            "h3",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/b/src/main.rs",
            &[node("crates/b/src/main.rs", None, "cb")],
            &[call(
                "crates/b/src/main.rs::cb",
                "connect",
                "crates/b/src/main.rs",
            )],
            "h4",
        )
        .unwrap();

        let foo_callers = get_callers(&conn, "crates/a/src/x.rs::Foo::connect");
        let names: Vec<&str> = foo_callers.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["ca"],
            "Foo::connect must only see crate-a caller"
        );

        let bar_callers = get_callers(&conn, "crates/b/src/y.rs::Bar::connect");
        let names: Vec<&str> = bar_callers.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["cb"],
            "Bar::connect must only see crate-b caller"
        );
    }

    /// Ultra-common names trust same-file evidence ONLY: even a unique
    /// `path` definition must not absorb `.path()` calls that actually
    /// target the stdlib.
    #[test]
    fn test_ultra_common_unique_definition_not_inflated() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(dir.path())).unwrap();

        store_file(
            &conn,
            "crates/core/src/cache.rs",
            &[
                node("crates/core/src/cache.rs", Some("Cache"), "path"),
                node("crates/core/src/cache.rs", None, "same_file_caller"),
            ],
            &[call(
                "crates/core/src/cache.rs::same_file_caller",
                "path",
                "crates/core/src/cache.rs",
            )],
            "h1",
        )
        .unwrap();
        // Unrelated file calling `.path()` (really tempfile::TempDir::path).
        store_file(
            &conn,
            "crates/core/src/other.rs",
            &[node("crates/core/src/other.rs", None, "uses_tempdir")],
            &[call(
                "crates/core/src/other.rs::uses_tempdir",
                "path",
                "crates/core/src/other.rs",
            )],
            "h2",
        )
        .unwrap();

        let callers = get_callers(&conn, "crates/core/src/cache.rs::Cache::path");
        assert!(
            !callers.iter().any(|n| n.name == "uses_tempdir"),
            "stdlib-shadowed .path() must not attribute cross-file: {:?}",
            callers.iter().map(|n| &n.name).collect::<Vec<_>>()
        );
        assert!(
            callers.iter().any(|n| n.name == "same_file_caller"),
            "same-file caller still counts"
        );
    }

    /// Typed-receiver edges (`Foo::new`) attribute to the right definition
    /// even from another crate.
    #[test]
    fn test_typed_receiver_edge_disambiguates() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(dir.path())).unwrap();

        store_file(
            &conn,
            "crates/a/src/x.rs",
            &[node("crates/a/src/x.rs", Some("Foo"), "new")],
            &[],
            "h1",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/b/src/y.rs",
            &[node("crates/b/src/y.rs", Some("Bar"), "new")],
            &[],
            "h2",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/c/src/main.rs",
            &[node("crates/c/src/main.rs", None, "cc")],
            &[call(
                "crates/c/src/main.rs::cc",
                "Foo::new",
                "crates/c/src/main.rs",
            )],
            "h3",
        )
        .unwrap();

        let foo_callers = get_callers(&conn, "crates/a/src/x.rs::Foo::new");
        assert_eq!(foo_callers.len(), 1);
        assert_eq!(foo_callers[0].name, "cc");
        assert!(get_callers(&conn, "crates/b/src/y.rs::Bar::new").is_empty());
    }

    /// An unresolvable bare `get` from an unrelated crate must not inflate
    /// caller counts — excluded by default, visible with the flag.
    #[test]
    fn test_unresolvable_bare_get_is_low_confidence() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(&project_in(dir.path())).unwrap();

        store_file(
            &conn,
            "crates/a/src/x.rs",
            &[node("crates/a/src/x.rs", Some("Cache"), "get")],
            &[],
            "h1",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/b/src/y.rs",
            &[node("crates/b/src/y.rs", Some("Store"), "get")],
            &[],
            "h2",
        )
        .unwrap();
        store_file(
            &conn,
            "crates/c/src/main.rs",
            &[node("crates/c/src/main.rs", None, "uses")],
            &[call(
                "crates/c/src/main.rs::uses",
                "get",
                "crates/c/src/main.rs",
            )],
            "h3",
        )
        .unwrap();

        assert!(
            get_callers(&conn, "crates/a/src/x.rs::Cache::get").is_empty(),
            "ambiguous ultra-common bare edge must not count by default"
        );
        let with_low = get_callers_opt(&conn, "crates/a/src/x.rs::Cache::get", true);
        assert_eq!(with_low.len(), 1, "explicit flag keeps it queryable");

        // Callees of the caller likewise drop the ambiguous bare target.
        assert!(get_callees(&conn, "crates/c/src/main.rs::uses").is_empty());
        assert_eq!(
            get_callees_opt(&conn, "crates/c/src/main.rs::uses", true).len(),
            2
        );
    }
}
