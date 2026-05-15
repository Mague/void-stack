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
    ImpactResult, get_callees, get_callers, get_impact_radius, get_tests_for, search_nodes,
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
    let root = PathBuf::from(strip_win_prefix(&project.path));
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
/// For projects rooted at `\\wsl.localhost\…`, `std::fs::read_dir` returns
/// silently empty in many process contexts on Windows. Detect that case
/// and shell out to `wsl.exe -- find` instead so the structural builder
/// actually sees the source files. The two paths return identical
/// project-relative POSIX paths.
fn collect_parseable_files(root: &Path) -> Vec<String> {
    if crate::fs_util::is_wsl_unc_path(root) {
        return collect_wsl_parseable_files(root);
    }
    use crate::ignore::VoidIgnore;
    let claudeignore = VoidIgnore::load_claudeignore(root);
    let voidignore = VoidIgnore::load(root);
    let mut out = Vec::new();
    walk(root, root, &mut out, &claudeignore, &voidignore, 8);
    out
}

/// File extensions tree-sitter knows about (mirrors `langs::language_for`).
/// Kept inline rather than re-exporting from `langs` so the find command
/// stays declarative and self-contained.
const WSL_FIND_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "jsx", "mjs", "ts", "tsx", "go", "dart", "java", "php", "phtml", "c", "h",
    "cpp", "cc", "cxx", "hpp",
];

/// Directory globs excluded from the WSL find — mirrors the in-process
/// `walk` filter so both paths return the same set.
const WSL_FIND_PRUNE: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    "dist",
    "build",
    "vendor",
    ".venv",
    "venv",
    ".next",
    "deps",
    "_build",
    ".git",
];

/// Walk a WSL-hosted project by shelling out to `wsl.exe -- find`. Returns
/// project-relative POSIX paths (`src/foo.rs`), matching what `walk` would
/// produce on a native path. Skips `.voidignore` / `.claudeignore`
/// filtering — those live on the Windows side and are loaded next.
fn collect_wsl_parseable_files(root: &Path) -> Vec<String> {
    use crate::fs_util::{unc_to_linux, wsl_exec};
    use crate::ignore::VoidIgnore;

    let Some(linux_root) = unc_to_linux(root) else {
        return Vec::new();
    };
    let args = build_wsl_find_args(&linux_root);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let Some(stdout) = wsl_exec(&arg_refs) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&stdout);

    // .voidignore / .claudeignore still apply — they live on the Windows
    // side under the UNC root, so the same loader works.
    let claudeignore = VoidIgnore::load_claudeignore(root);
    let voidignore = VoidIgnore::load(root);

    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Strip the linux root prefix so callers get a project-relative
        // POSIX path. Tolerate find returning either `<root>/...` or just
        // `./...`-style relatives.
        let rel = line
            .strip_prefix(&linux_root)
            .map(|s| s.trim_start_matches('/'))
            .unwrap_or(line)
            .to_string();
        if claudeignore.is_ignored(&rel) || voidignore.is_ignored(&rel) {
            continue;
        }
        out.push(rel);
    }
    out
}

/// Build the `wsl.exe` argv for the find walker. Extracted from
/// `collect_wsl_parseable_files` so the argv shape is unit-testable
/// without spawning `wsl.exe`.
pub(crate) fn build_wsl_find_args(linux_root: &str) -> Vec<String> {
    let mut args: Vec<String> = vec!["--".into(), "find".into(), linux_root.into()];

    // Prune unwanted dirs before descending into them — much faster than
    // -not -path after-the-fact, and keeps the result small enough that
    // a single round-trip stays under wsl.exe's argv limits.
    //
    // Parentheses must be escaped (`\(` / `\)`): wsl.exe reconstructs the
    // argv as a shell command line, and bare `(` / `)` would otherwise
    // open a subshell instead of grouping find expressions — making find
    // exit non-zero and the whole walker silently return zero files.
    if !WSL_FIND_PRUNE.is_empty() {
        args.push("\\(".into());
        for (i, name) in WSL_FIND_PRUNE.iter().enumerate() {
            if i > 0 {
                args.push("-o".into());
            }
            args.push("-name".into());
            args.push((*name).into());
        }
        args.push("\\)".into());
        args.push("-prune".into());
        args.push("-o".into());
    }

    args.push("-type".into());
    args.push("f".into());

    if !WSL_FIND_EXTENSIONS.is_empty() {
        args.push("\\(".into());
        for (i, ext) in WSL_FIND_EXTENSIONS.iter().enumerate() {
            if i > 0 {
                args.push("-o".into());
            }
            args.push("-name".into());
            args.push(format!("*.{}", ext));
        }
        args.push("\\)".into());
    }

    args.push("-print".into());
    args
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
    fn test_build_wsl_find_args_includes_root_and_extensions() {
        let args = build_wsl_find_args("/home/user/project");
        assert_eq!(args[0], "--");
        assert_eq!(args[1], "find");
        assert_eq!(args[2], "/home/user/project");
        // Should -prune common vendored dirs (node_modules, target, etc.)
        // before walking into them.
        assert!(args.iter().any(|a| a == "node_modules"));
        assert!(args.iter().any(|a| a == "target"));
        assert!(args.iter().any(|a| a == "-prune"));
        // Should match the supported extensions via `-name *.rs`, etc.
        assert!(args.iter().any(|a| a == "*.rs"));
        assert!(args.iter().any(|a| a == "*.ts"));
        assert!(args.iter().any(|a| a == "*.tsx"));
        assert!(args.iter().any(|a| a == "*.go"));
        assert!(args.iter().any(|a| a == "*.py"));
        // Filter to regular files only, and emit one path per line.
        assert!(args.iter().any(|a| a == "-type"));
        assert!(args.iter().any(|a| a == "f"));
        assert!(args.last().is_some_and(|a| a == "-print"));
        // Parens MUST be backslash-escaped so wsl.exe -> bash doesn't
        // treat them as subshell operators. Bare `(` / `)` would make
        // find exit non-zero and zero out the entire walker.
        assert!(
            args.iter().any(|a| a == "\\("),
            "expected escaped opening paren in {args:?}"
        );
        assert!(
            args.iter().any(|a| a == "\\)"),
            "expected escaped closing paren in {args:?}"
        );
        assert!(
            !args.iter().any(|a| a == "(" || a == ")"),
            "bare parens leaked into argv: {args:?}"
        );
    }

    #[test]
    fn test_build_wsl_find_args_balances_parens() {
        let args = build_wsl_find_args("/x");
        let opens = args.iter().filter(|a| *a == "\\(").count();
        let closes = args.iter().filter(|a| *a == "\\)").count();
        assert_eq!(opens, closes, "parens must be balanced: {args:?}");
        // Sanity: there are *some* groups to balance — two pairs, one
        // for prune and one for the extension filter.
        assert!(opens >= 2, "expected ≥2 groups, got {opens}");
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
