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

/// Functions that call `qualified_name` (reverse edges).
pub fn get_callers(conn: &Connection, qualified_name: &str) -> Vec<StructuralNode> {
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT source_qualified FROM edges \
         WHERE target_qualified = ?1 AND kind = 'CALLS'",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let qns: Vec<String> = match stmt.query_map([qualified_name], |r| r.get::<_, String>(0)) {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => return Vec::new(),
    };
    nodes_by_qnames(conn, &qns).unwrap_or_default()
}

/// Functions called by `qualified_name` (forward edges).
pub fn get_callees(conn: &Connection, qualified_name: &str) -> Vec<StructuralNode> {
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT target_qualified FROM edges \
         WHERE source_qualified = ?1 AND kind = 'CALLS'",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let qns: Vec<String> = match stmt.query_map([qualified_name], |r| r.get::<_, String>(0)) {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => return Vec::new(),
    };
    nodes_by_qnames(conn, &qns).unwrap_or_default()
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
pub fn search_nodes(conn: &Connection, query: &str, limit: usize) -> Vec<StructuralNode> {
    let like = format!("%{}%", query);
    let mut stmt = match conn.prepare(
        "SELECT kind, name, qualified_name, file_path, line_start, line_end, \
         language, parent_name, is_test FROM nodes \
         WHERE name LIKE ?1 OR qualified_name LIKE ?1 LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(rusqlite::params![like, limit as i64], |row| {
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
    }) {
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
