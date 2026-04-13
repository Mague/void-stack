//! SQLite storage for the structural graph.
//!
//! The DB lives at `.void-stack/structural.db` inside the project. Schema
//! mirrors code-review-graph: one table for nodes, one for edges, with
//! `qualified_name` UNIQUE for idempotent upserts.

use std::path::PathBuf;

use chrono::Utc;
use rusqlite::Connection;

use super::parser::{NodeKind, StructuralEdge, StructuralNode};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Path to the project-local structural DB.
pub fn structural_db_path(project: &Project) -> PathBuf {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    root.join(".void-stack").join("structural.db")
}

/// Open (creating if needed) the structural graph DB for a project and
/// ensure the schema is up to date.
pub fn open_db(project: &Project) -> Result<Connection, String> {
    let path = structural_db_path(project);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
    }
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            qualified_name TEXT NOT NULL UNIQUE,
            file_path TEXT NOT NULL,
            line_start INTEGER NOT NULL DEFAULT 0,
            line_end INTEGER NOT NULL DEFAULT 0,
            language TEXT NOT NULL DEFAULT '',
            parent_name TEXT,
            is_test INTEGER NOT NULL DEFAULT 0,
            file_hash TEXT NOT NULL DEFAULT '',
            updated_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            source_qualified TEXT NOT NULL,
            target_qualified TEXT NOT NULL,
            file_path TEXT NOT NULL,
            line INTEGER NOT NULL DEFAULT 0,
            updated_at REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(file_path);
        CREATE INDEX IF NOT EXISTS idx_nodes_qname ON nodes(qualified_name);
        CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_qualified);
        CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_qualified);
        CREATE INDEX IF NOT EXISTS idx_edges_file ON edges(file_path);
        CREATE TABLE IF NOT EXISTS stats (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

/// Replace all nodes and edges for a file atomically. Any previous rows
/// tied to `file_path` are removed first.
pub fn store_file(
    conn: &Connection,
    file_path: &str,
    nodes: &[StructuralNode],
    edges: &[StructuralEdge],
    file_hash: &str,
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let ts = Utc::now().timestamp() as f64;

    tx.execute("DELETE FROM nodes WHERE file_path = ?1", [file_path])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM edges WHERE file_path = ?1", [file_path])
        .map_err(|e| e.to_string())?;

    {
        let mut insert_node = tx
            .prepare(
                "INSERT OR REPLACE INTO nodes \
                 (kind, name, qualified_name, file_path, line_start, line_end, \
                  language, parent_name, is_test, file_hash, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            .map_err(|e| e.to_string())?;

        for n in nodes {
            insert_node
                .execute(rusqlite::params![
                    n.kind.as_str(),
                    n.name,
                    n.qualified_name,
                    n.file_path,
                    n.line_start as i64,
                    n.line_end as i64,
                    n.language,
                    n.parent_name,
                    if n.is_test { 1i64 } else { 0i64 },
                    file_hash,
                    ts,
                ])
                .map_err(|e| e.to_string())?;
        }

        let mut insert_edge = tx
            .prepare(
                "INSERT INTO edges \
                 (kind, source_qualified, target_qualified, file_path, line, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|e| e.to_string())?;
        for e in edges {
            insert_edge
                .execute(rusqlite::params![
                    e.kind.as_str(),
                    e.source_qualified,
                    e.target_qualified,
                    e.file_path,
                    e.line as i64,
                    ts,
                ])
                .map_err(|err| err.to_string())?;
        }
    }

    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// Delete every row tied to a file.
pub fn remove_file(conn: &Connection, file_path: &str) -> Result<(), String> {
    conn.execute("DELETE FROM nodes WHERE file_path = ?1", [file_path])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM edges WHERE file_path = ?1", [file_path])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Return the stored `file_hash` (empty string if unknown).
pub fn get_file_hash(conn: &Connection, file_path: &str) -> Option<String> {
    conn.query_row(
        "SELECT file_hash FROM nodes WHERE file_path = ?1 AND file_hash != '' LIMIT 1",
        [file_path],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Fetch every node whose row-id matches `qualified_name`s in the slice.
pub fn nodes_by_qnames(
    conn: &Connection,
    qnames: &[String],
) -> Result<Vec<StructuralNode>, String> {
    if qnames.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for chunk in qnames.chunks(900) {
        let placeholders: Vec<String> = (0..chunk.len()).map(|i| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT kind, name, qualified_name, file_path, line_start, line_end, \
             language, parent_name, is_test FROM nodes WHERE qualified_name IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
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
            })
            .map_err(|e| e.to_string())?;
        for r in rows.flatten() {
            out.push(r);
        }
    }
    Ok(out)
}

/// Total node count — useful for stats and smoke tests.
pub fn count_nodes(conn: &Connection) -> Result<usize, String> {
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(n as usize)
}

/// Total edge count.
pub fn count_edges(conn: &Connection) -> Result<usize, String> {
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(n as usize)
}

/// Fetch all qualified names in a given set of files. Used to build the
/// seed set for impact-radius queries.
pub fn qnames_in_files(conn: &Connection, files: &[String]) -> Result<Vec<String>, String> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for chunk in files.chunks(900) {
        let placeholders: Vec<String> = (0..chunk.len()).map(|i| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT qualified_name FROM nodes WHERE file_path IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;
        for r in rows.flatten() {
            out.push(r);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::parser::EdgeKind;
    use super::*;
    use crate::model::Project;

    fn project_in(dir: &std::path::Path) -> Project {
        Project {
            name: "sg-test".to_string(),
            path: dir.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    #[test]
    fn test_open_db_creates_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_in(tmp.path());
        let conn = open_db(&project).unwrap();
        // Sanity-check the schema: both tables exist.
        let names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .flatten()
            .collect();
        assert!(names.contains(&"nodes".to_string()));
        assert!(names.contains(&"edges".to_string()));
    }

    #[test]
    fn test_store_and_retrieve() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_in(tmp.path());
        let conn = open_db(&project).unwrap();

        let nodes = vec![StructuralNode {
            kind: NodeKind::Function,
            name: "foo".to_string(),
            qualified_name: "a.rs::foo".to_string(),
            file_path: "a.rs".to_string(),
            line_start: 1,
            line_end: 2,
            language: "rust".to_string(),
            parent_name: None,
            is_test: false,
        }];
        let edges = vec![StructuralEdge {
            kind: EdgeKind::Calls,
            source_qualified: "a.rs::foo".to_string(),
            target_qualified: "a.rs::bar".to_string(),
            file_path: "a.rs".to_string(),
            line: 2,
        }];
        store_file(&conn, "a.rs", &nodes, &edges, "abc").unwrap();

        assert_eq!(count_nodes(&conn).unwrap(), 1);
        assert_eq!(count_edges(&conn).unwrap(), 1);
        assert_eq!(get_file_hash(&conn, "a.rs").as_deref(), Some("abc"));

        let got = nodes_by_qnames(&conn, &vec!["a.rs::foo".to_string()]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "foo");
    }

    #[test]
    fn test_store_replaces_previous_file_data() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_in(tmp.path());
        let conn = open_db(&project).unwrap();

        let n1 = vec![StructuralNode {
            kind: NodeKind::Function,
            name: "old".to_string(),
            qualified_name: "a.rs::old".to_string(),
            file_path: "a.rs".to_string(),
            line_start: 1,
            line_end: 1,
            language: "rust".to_string(),
            parent_name: None,
            is_test: false,
        }];
        store_file(&conn, "a.rs", &n1, &[], "h1").unwrap();

        let n2 = vec![StructuralNode {
            kind: NodeKind::Function,
            name: "new".to_string(),
            qualified_name: "a.rs::new".to_string(),
            file_path: "a.rs".to_string(),
            line_start: 1,
            line_end: 1,
            language: "rust".to_string(),
            parent_name: None,
            is_test: false,
        }];
        store_file(&conn, "a.rs", &n2, &[], "h2").unwrap();

        assert_eq!(count_nodes(&conn).unwrap(), 1);
        assert_eq!(get_file_hash(&conn, "a.rs").as_deref(), Some("h2"));
    }

    #[test]
    fn test_remove_file_deletes_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_in(tmp.path());
        let conn = open_db(&project).unwrap();

        let nodes = vec![StructuralNode {
            kind: NodeKind::Function,
            name: "foo".to_string(),
            qualified_name: "a.rs::foo".to_string(),
            file_path: "a.rs".to_string(),
            line_start: 1,
            line_end: 1,
            language: "rust".to_string(),
            parent_name: None,
            is_test: false,
        }];
        store_file(&conn, "a.rs", &nodes, &[], "h").unwrap();
        remove_file(&conn, "a.rs").unwrap();
        assert_eq!(count_nodes(&conn).unwrap(), 0);
    }
}
