//! The project's own source files: what a map or an entry-point scan
//! should look at, skipping dependencies, build output and ignored paths.

use std::path::Path;

use crate::ignore::VoidIgnore;

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "obj",
    "__pycache__",
    "venv",
    "env",
    "vendor",
    "deps",
    "_build",
    "coverage",
    "site-packages",
    "bower_components",
    "Pods",
    "DerivedData",
    "generated",
    // void-stack's own output folder (graph.html, benchmarks, worktrees).
    "void-stack-out",
];

/// Deepest directory level walked.
const MAX_DEPTH: usize = 9;
/// Walking stops after this many files (monorepos with vendored trees).
pub(crate) const MAX_FILES: usize = 60_000;

/// One file of the project, path relative to the root with `/`.
#[derive(Debug, Clone)]
pub(crate) struct SourceFile {
    pub rel: String,
    pub size: u64,
}

/// Every non-ignored file under `root` (code, docs and manifests alike).
pub(crate) fn walk(root: &Path) -> Vec<SourceFile> {
    let voidignore = VoidIgnore::load(root);
    let claudeignore = VoidIgnore::load_claudeignore(root);
    let mut out = Vec::new();
    walk_dir(root, root, 0, &voidignore, &claudeignore, &mut out);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    depth: usize,
    voidignore: &VoidIgnore,
    claudeignore: &VoidIgnore,
    out: &mut Vec<SourceFile>,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            // A git worktree of this same repo inside the project duplicates
            // every file. Submodules and vendored clones stay: they are part
            // of the project (0.35 skipped any dir with a `.git`, which hid
            // `clients/claude-code` in seosnap).
            if is_git_worktree(&path) {
                continue;
            }
            if name.starts_with('.')
                || SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s))
                || voidignore.is_generated_dir(&name)
                || voidignore.is_ignored(&format!("{rel}/"))
                || claudeignore.is_ignored(&format!("{rel}/"))
            {
                continue;
            }
            walk_dir(root, &path, depth + 1, voidignore, claudeignore, out);
        } else if ft.is_file() {
            if name == ".git" || voidignore.is_ignored(&rel) || claudeignore.is_ignored(&rel) {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(SourceFile { rel, size });
            if out.len() >= MAX_FILES {
                return;
            }
        }
    }
}

/// `dir/.git` is a file pointing into another repo's `.git/worktrees/`.
fn is_git_worktree(dir: &Path) -> bool {
    let dot_git = dir.join(".git");
    dot_git.is_file()
        && std::fs::read_to_string(&dot_git)
            .map(|t| t.replace('\\', "/").contains("/worktrees/"))
            .unwrap_or(false)
}

/// Language of a source file by extension; `None` for non-code files.
pub(crate) fn language(rel: &str) -> Option<&'static str> {
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "rs" => "Rust",
        "py" => "Python",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "go" => "Go",
        "dart" => "Dart",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "cs" => "C#",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "ex" | "exs" => "Elixir",
        "php" => "PHP",
        "rb" => "Ruby",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "proto" => "Protobuf",
        "gd" => "GDScript",
        "mq4" | "mq5" | "mqh" => "MQL",
        "sql" => "SQL",
        "lua" => "Lua",
        "scala" => "Scala",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_skips_dependencies_and_hidden_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        for p in [
            "src/main.rs",
            "node_modules/x/index.js",
            "target/debug/a.rs",
            ".git/config",
            "web/src/app.ts",
            "README.md",
            "out-of-tree/wt/src/main.rs",
            "clients/tool/.git",
            "clients/tool/index.ts",
        ] {
            let f = r.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, "x").unwrap();
        }
        // A worktree's `.git` file points into `.git/worktrees/`; a
        // submodule's into `.git/modules/` (kept).
        std::fs::write(
            r.join("out-of-tree/wt/.git"),
            "gitdir: F:\\repo\\.git\\worktrees\\wt\n",
        )
        .unwrap();
        std::fs::write(
            r.join("clients/tool/.git"),
            "gitdir: ../../.git/modules/tool\n",
        )
        .unwrap();
        let files: Vec<String> = walk(r).into_iter().map(|f| f.rel).collect();
        assert_eq!(
            files,
            vec![
                "README.md",
                "clients/tool/index.ts",
                "src/main.rs",
                "web/src/app.ts"
            ]
        );
    }

    #[test]
    fn languages_by_extension() {
        assert_eq!(language("a/b.rs"), Some("Rust"));
        assert_eq!(language("x.TSX"), Some("TypeScript"));
        assert_eq!(language("README.md"), None);
        assert_eq!(language("Makefile"), None);
    }
}
