//! Structural graph: tree-sitter powered function-level call graph.
//!
//! Behind the `structural` feature. Exposes a small API:
//! - [`parse_file`] parses one file into nodes + edges.
//! - [`build_structural_graph`] walks a project, incrementally populating
//!   a SQLite DB rooted at `.void-stack/structural.db`.
//! - Query helpers ([`get_impact_radius`], [`get_callers`], ...) run
//!   SQLite queries over that DB.

pub mod graph;
pub(crate) mod langs;
pub mod model;
pub mod parser;
pub mod query;

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;

pub use graph::{
    count_edges, count_nodes, get_file_hash, nodes_by_qnames, open_db, qnames_in_files,
    remove_file, store_file, structural_db_path,
};
pub use parser::{
    EdgeKind, NodeKind, ParseResult, StructuralEdge, StructuralNode, is_test_function,
    language_for, parse_file, parse_file_with_rel, qualify,
};
pub use query::{
    ImpactResult, get_callees, get_callees_opt, get_callers, get_callers_opt, get_impact_radius,
    get_tests_for, search_nodes,
};

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

#[derive(Debug, Clone, Serialize)]
pub struct StructuralStats {
    pub files_parsed: usize,
    pub files_skipped: usize,
    pub nodes_total: usize,
    pub edges_total: usize,
    pub built_at: chrono::DateTime<Utc>,
}

use crate::fs_util::file_sha256;

/// Build or incrementally update the structural graph for a project.
/// Skips files whose SHA-256 matches the stored one unless `force` is set.
#[cfg(feature = "structural")]
pub fn build_structural_graph(project: &Project, force: bool) -> Result<StructuralStats, String> {
    let raw_root = strip_win_prefix(&project.path);
    // Forward-slash UNC roots (`//wsl.localhost/<distro>/…`) work with
    // `std::fs::read_dir` / `read`, the backslash form does not. Normalise
    // once at the top so every downstream join inherits the working form
    // and we never round-trip through `wsl.exe -- cat` for file reads.
    let root = if is_wsl_unc_path_str(&raw_root) {
        PathBuf::from(raw_root.replace('\\', "/"))
    } else {
        PathBuf::from(raw_root)
    };
    let files = collect_parseable_files(&root);

    let conn = open_db(project)?;
    if force {
        conn.execute_batch("DELETE FROM nodes; DELETE FROM edges;")
            .map_err(|e| e.to_string())?;
    }

    let mut parsed = 0usize;
    let mut skipped = 0usize;

    for rel in &files {
        let abs = root.join(rel);
        let hash = file_sha256(&abs);

        if !force
            && !hash.is_empty()
            && let Some(prev) = get_file_hash(&conn, rel)
            && prev == hash
        {
            skipped += 1;
            continue;
        }

        let Some(result) = parse_file_with_rel(&abs, Some(rel)) else {
            continue;
        };
        store_file(&conn, rel, &result.nodes, &result.edges, &hash)?;
        parsed += 1;
    }

    Ok(StructuralStats {
        files_parsed: parsed,
        files_skipped: skipped,
        nodes_total: count_nodes(&conn)?,
        edges_total: count_edges(&conn)?,
        built_at: Utc::now(),
    })
}

#[cfg(not(feature = "structural"))]
pub fn build_structural_graph(_project: &Project, _force: bool) -> Result<StructuralStats, String> {
    Err("structural feature not enabled".to_string())
}

/// Collect files with extensions tree-sitter can parse. Respects
/// `.voidignore` and `.claudeignore`, and skips the usual vendored dirs.
///
/// Windows 11 + WSL2 exposes the WSL filesystem to Win32 via the 9P
/// redirector, so `std::fs::read_dir` works on the `//wsl.localhost/…`
/// form (verified at runtime on Windows 11 23H2). The `\\wsl.localhost\…`
/// backslash form fails with `OS error 3` — a long-standing quirk of
/// Rust's Windows path handling on 9P-backed UNC roots. We normalise
/// `\\` → `//` for any UNC root before walking so the same registry
/// entry works in both spellings.
fn collect_parseable_files(root: &Path) -> Vec<String> {
    use crate::ignore::VoidIgnore;

    let root_str = root.to_string_lossy();
    let normalised: PathBuf;
    let walk_root: &Path = if is_wsl_unc_path_str(&root_str) {
        normalised = PathBuf::from(root_str.replace('\\', "/"));
        normalised.as_path()
    } else {
        root
    };

    let claudeignore = VoidIgnore::load_claudeignore(walk_root);
    let voidignore = VoidIgnore::load(walk_root);
    let mut out = Vec::new();
    walk(
        walk_root,
        walk_root,
        &mut out,
        &claudeignore,
        &voidignore,
        8,
    );
    out
}

/// String-form WSL UNC check kept local to the structural module so we
/// don't have to widen `fs_util::is_wsl_unc_path`'s API. Mirrors the
/// same prefix recognition (`\\wsl…`, `//wsl…`, case-insensitive).
fn is_wsl_unc_path_str(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.starts_with(r"\\wsl") || lower.starts_with("//wsl")
}

fn walk(
    root: &Path,
    current: &Path,
    out: &mut Vec<String>,
    claudeignore: &crate::ignore::VoidIgnore,
    voidignore: &crate::ignore::VoidIgnore,
    depth: u32,
) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.')
            || matches!(
                name.as_str(),
                "node_modules"
                    | "target"
                    | "__pycache__"
                    | "dist"
                    | "build"
                    | "vendor"
                    | ".venv"
                    | "venv"
                    | ".next"
                    | ".nuxt"
                    | ".dart_tool"
                    | ".turbo"
                    | "coverage"
            )
        {
            continue;
        }

        if let Ok(rel) = path.strip_prefix(root) {
            let rel_str = rel.to_string_lossy().to_string();
            if claudeignore.is_ignored(&rel_str) || voidignore.is_ignored(&rel_str) {
                continue;
            }
        }

        if path.is_dir() {
            walk(root, &path, out, claudeignore, voidignore, depth - 1);
        } else if path.is_file()
            && language_for(&path).is_some()
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[cfg(all(test, feature = "structural"))]
mod tests {
    use super::*;

    fn project_in(dir: &std::path::Path) -> Project {
        Project {
            name: "sg-int".to_string(),
            path: dir.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    #[test]
    fn test_build_structural_graph_rust() {
        let tmp = tempfile::tempdir().unwrap();
        // Two Rust files; b.rs defines foo, a.rs calls it.
        std::fs::write(tmp.path().join("b.rs"), "pub fn foo() -> i32 { 1 }\n").unwrap();
        std::fs::write(
            tmp.path().join("a.rs"),
            "fn run() {\n    let _ = crate::b::foo();\n}\n",
        )
        .unwrap();

        let stats = build_structural_graph(&project_in(tmp.path()), false).unwrap();
        assert!(stats.files_parsed >= 2);
        assert!(stats.nodes_total > 0);
        assert!(stats.edges_total > 0);
    }

    #[test]
    fn test_build_structural_graph_incremental_skips_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "pub fn foo() -> i32 { 1 }\n").unwrap();

        let s1 = build_structural_graph(&project_in(tmp.path()), false).unwrap();
        assert!(s1.files_parsed >= 1);

        let s2 = build_structural_graph(&project_in(tmp.path()), false).unwrap();
        assert_eq!(s2.files_parsed, 0, "nothing should re-parse");
        assert!(s2.files_skipped >= 1);
    }

    #[test]
    fn test_build_structural_graph_force_reparses() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "pub fn foo() -> i32 { 1 }\n").unwrap();
        let _ = build_structural_graph(&project_in(tmp.path()), false).unwrap();
        let s = build_structural_graph(&project_in(tmp.path()), true).unwrap();
        assert!(s.files_parsed >= 1, "force should always re-parse");
    }

    #[test]
    fn test_structural_db_path_wsl_routes_to_appdata() {
        // For a UNC WSL project the DB must NOT live inside the
        // (unreadable-by-rusqlite) UNC root — it goes to AppData.
        let project = Project {
            name: "wsl-demo".to_string(),
            path: r"\\wsl.localhost\Ubuntu-24.04\home\user\project".to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let path = graph::structural_db_path(&project);
        let s = path.to_string_lossy();
        assert!(
            !s.starts_with(r"\\wsl"),
            "DB must not live on the UNC path, got {s}"
        );
        assert!(s.contains("void-stack"), "got {s}");
        assert!(s.contains("structural"), "got {s}");
        assert!(s.contains("wsl-demo"), "got {s}");
        assert!(s.ends_with("structural.db"), "got {s}");
    }

    #[test]
    fn test_structural_db_path_local_stays_in_project() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_in(tmp.path());
        let path = graph::structural_db_path(&project);
        // For native paths the DB must stay alongside the source, NOT in
        // AppData — otherwise CI / repo-relative tooling breaks.
        let p = path.to_string_lossy().to_string();
        assert!(p.contains(".void-stack"), "got {p}");
        assert!(p.ends_with("structural.db"), "got {p}");
    }
}
