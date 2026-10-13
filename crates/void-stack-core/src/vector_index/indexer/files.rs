//! Which files of a project get indexed.

use std::path::Path;

use crate::ignore::VoidIgnore;
use crate::security::is_sensitive_file;

/// Source code extensions worth indexing.
/// Source-code extensions indexed for semantic search.
///
/// Deliberately excludes config/data files (`json`, `yaml`, `yml`, `toml`)
/// and shell-like scripts (`sh`, `bash`, `zsh`, `fish`, `ps1`, `bat`,
/// `cmd`) — they dominate the index with package-lock / schema / CI
/// boilerplate that drowns real code in search results. Kept: source
/// languages, `md` for READMEs, `proto`/`sql` for typed code, and the
/// `dockerfile` build spec.
pub(crate) const CODE_EXTENSIONS: &[&str] = &[
    "rs",
    "go",
    "py",
    "js",
    "ts",
    "tsx",
    "jsx",
    "java",
    "kt",
    "swift",
    "dart",
    "c",
    "cpp",
    "h",
    "hpp",
    "mq4",
    "mq5",
    "mqh",
    "cs",
    "rb",
    "php",
    "lua",
    "zig",
    "ex",
    "exs",
    "vue",
    "svelte",
    "astro",
    "proto",
    "sql",
    "md",
    "dockerfile",
    "verse",
    // Godot: GDScript (C# is `cs` above)
    "gd",
];

/// Max file size to index (500KB).
pub(super) const MAX_FILE_SIZE: u64 = 500_000;

pub(crate) fn collect_indexable_files(project_path: &Path) -> Vec<String> {
    // Load ignore patterns
    let claudeignore = VoidIgnore::load_claudeignore(project_path);
    let voidignore = VoidIgnore::load(project_path);

    let mut files = Vec::new();
    collect_files_recursive(
        project_path,
        project_path,
        &mut files,
        &claudeignore,
        &voidignore,
        6, // deeper than file_reader's 3 levels
    );
    files
}

fn collect_files_recursive(
    root: &Path,
    current: &Path,
    files: &mut Vec<String>,
    claudeignore: &VoidIgnore,
    voidignore: &VoidIgnore,
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
        if is_skipped_entry(&name, voidignore) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        // Always `/`: stored paths are matched against the import graph,
        // git's changed-file list and ignore patterns, which all use `/`.
        // With Windows' `\` none of them matched — import context was never
        // embedded on Windows and nested ignore patterns didn't apply.
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if claudeignore.is_ignored(&rel_str) || voidignore.is_ignored(&rel_str) {
            continue;
        }

        if path.is_dir() {
            collect_files_recursive(root, &path, files, claudeignore, voidignore, depth - 1);
        } else if path.is_file() && is_indexable_file(&path, &name) {
            files.push(rel_str);
        }
    }
}

/// Hidden entries, dependency/build/cache folders, and engine-generated
/// folders.
fn is_skipped_entry(name: &str, voidignore: &VoidIgnore) -> bool {
    name.starts_with('.') || SKIPPED_DIRS.contains(&name) || voidignore.is_generated_dir(name)
}

const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    "dist",
    "build",
    "vendor",
    "venv",
    "coverage",
    // Unreal Engine / UEFN generated dirs (Plugins/ is kept)
    "Intermediate",
    "Saved",
    "Binaries",
    "DerivedDataCache",
];

/// Generated sources and coverage reports that look like code.
const GENERATED_SUFFIXES: &[&str] = &[
    ".pb.rs",
    "_pb2.py",
    ".pb.go",
    ".g.dart",
    ".freezed.dart",
    ".gen.go",
];
const GENERATED_FILES: &[&str] = &["lcov.info", "coverage.xml"];

/// Source file worth indexing: a code extension (or a known file name like
/// `Dockerfile`), not generated, not sensitive, not over `MAX_FILE_SIZE`.
fn is_indexable_file(path: &Path, name: &str) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let is_code = CODE_EXTENSIONS.contains(&ext.as_str())
        || CODE_EXTENSIONS.contains(&name.to_lowercase().as_str());
    is_code
        && !GENERATED_SUFFIXES.iter().any(|s| name.ends_with(s))
        && !GENERATED_FILES.contains(&name)
        && !is_sensitive_file(path)
        && path
            .metadata()
            .map(|m| m.len() <= MAX_FILE_SIZE)
            .unwrap_or(true)
}

#[cfg(test)]
mod path_tests {
    use super::*;

    /// Stored paths use `/` on every OS (on Windows they used to keep `\`,
    /// so they never matched the import graph or git's file list).
    #[test]
    fn indexable_files_use_forward_slashes() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("app").join("api").join("handlers.py");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "def handle():\n    return 1\n").unwrap();
        let files = collect_indexable_files(dir.path());
        assert_eq!(files, vec!["app/api/handlers.py".to_string()]);
    }
}
