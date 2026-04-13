//! Import parsing for multiple languages.

mod classifier;
pub mod dart;
pub mod golang;
pub mod javascript;
pub mod python;
pub mod rust_lang;

use std::path::Path;

use super::graph::*;
use crate::security;

/// Result of parsing a single file.
#[derive(Default)]
pub struct ParseResult {
    pub imports: Vec<RawImport>,
    pub class_count: usize,
    pub function_count: usize,
    pub loc: usize,
    /// True when the file is mostly re-exports (`pub use` / `pub mod`) —
    /// a hub, not a god class. Defaults to `false` (language parsers that
    /// don't set it just never hit the hub exception).
    pub is_hub: bool,
    /// True when the file uses framework macros that force one function
    /// per tool/handler (currently: rmcp `#[tool_router]` / `#[tool_handler]`).
    /// God-class detection should ignore the function_count trigger for
    /// these files since the count is inherent to the pattern.
    pub has_framework_macros: bool,
}

/// A raw import found in source code.
pub struct RawImport {
    pub module_path: String,
    pub is_relative: bool,
}

/// Trait for language-specific import parsers.
pub trait ImportParser {
    fn language(&self) -> Language;
    fn file_extensions(&self) -> &[&str];
    fn parse_file(&self, content: &str, file_path: &str) -> ParseResult;
}

/// Directories to skip during scanning.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".venv",
    "venv",
    "env",
    "__pycache__",
    ".git",
    "target",
    "build",
    "dist",
    ".next",
    ".nuxt",
    "coverage",
    ".tox",
    "vendor",
    "eggs",
    ".eggs",
    ".mypy_cache",
    ".pytest_cache",
];

/// Detect the primary language for a directory.
pub fn detect_language(dir: &Path) -> Option<Language> {
    if dir.join("requirements.txt").exists()
        || dir.join("pyproject.toml").exists()
        || dir.join("setup.py").exists()
    {
        return Some(Language::Python);
    }
    // Fallback: check if there are .py files directly in the dir
    if let Ok(entries) = std::fs::read_dir(dir) {
        let has_py = entries
            .flatten()
            .any(|e| e.path().extension().map(|ext| ext == "py").unwrap_or(false));
        if has_py {
            return Some(Language::Python);
        }
    }
    if dir.join("package.json").exists() {
        // Check if TS
        if dir.join("tsconfig.json").exists() {
            return Some(Language::TypeScript);
        }
        return Some(Language::JavaScript);
    }
    if dir.join("go.mod").exists() {
        return Some(Language::Go);
    }
    if dir.join("pubspec.yaml").exists() {
        return Some(Language::Dart);
    }
    if dir.join("Cargo.toml").exists() {
        return Some(Language::Rust);
    }
    None
}

/// Build a dependency graph for a project directory.
pub fn build_graph(dir: &Path) -> Option<DependencyGraph> {
    let lang = detect_language(dir)?;

    let parsers: Vec<Box<dyn ImportParser>> = match lang {
        Language::Python => vec![Box::new(python::PythonParser)],
        Language::JavaScript | Language::TypeScript => vec![Box::new(javascript::JsParser)],
        Language::Go => vec![Box::new(golang::GoParser)],
        Language::Dart => vec![Box::new(dart::DartParser)],
        Language::Rust => vec![Box::new(rust_lang::RustParser)],
    };

    let mut modules: Vec<ModuleNode> = Vec::new();
    let mut edges: Vec<ImportEdge> = Vec::new();
    let mut external_deps: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Walk the directory (respecting .voidignore)
    let dir_str = dir.to_string_lossy().replace('\\', "/");
    let ignore = crate::ignore::VoidIgnore::load(dir);
    let mut file_paths: Vec<(String, String)> = Vec::new(); // (abs_path, rel_path)
    collect_files(dir, dir, &parsers, &ignore, &mut file_paths);

    // Known project modules (for resolving internal vs external)
    let known_modules: std::collections::HashSet<String> =
        file_paths.iter().map(|(_, rel)| rel.clone()).collect();

    for (abs_path, rel_path) in &file_paths {
        let content = match std::fs::read_to_string(abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let parser = parsers.iter().find(|p| {
            p.file_extensions()
                .iter()
                .any(|ext| rel_path.ends_with(ext))
        });
        let parser = match parser {
            Some(p) => p,
            None => continue,
        };

        let result = parser.parse_file(&content, rel_path);
        let layer = classifier::classify_layer(rel_path, &content);

        modules.push(ModuleNode {
            path: rel_path.clone(),
            language: parser.language(),
            layer,
            loc: result.loc,
            class_count: result.class_count,
            function_count: result.function_count,
            is_hub: result.is_hub,
            has_framework_macros: result.has_framework_macros,
        });

        for imp in &result.imports {
            let resolved = resolve_import(
                &imp.module_path,
                rel_path,
                imp.is_relative,
                &known_modules,
                parser.language(),
            );
            let is_external = resolved.is_none() && !imp.is_relative;

            if is_external {
                let pkg = imp
                    .module_path
                    .split('/')
                    .next()
                    .or_else(|| imp.module_path.split('.').next())
                    .unwrap_or(&imp.module_path);
                external_deps.insert(pkg.to_string());
            }

            edges.push(ImportEdge {
                from: rel_path.clone(),
                to: resolved.unwrap_or_else(|| imp.module_path.clone()),
                is_external,
            });
        }
    }

    // Fan-in/fan-out refinement for remaining Unknown modules
    classifier::refine_unknown_by_graph(&mut modules, &edges);

    Some(DependencyGraph {
        root_path: dir_str,
        primary_language: lang,
        modules,
        edges,
        external_deps,
    })
}

fn collect_files(
    base: &Path,
    dir: &Path,
    parsers: &[Box<dyn ImportParser>],
    ignore: &crate::ignore::VoidIgnore,
    out: &mut Vec<(String, String)>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            // Check .voidignore for directories
            if let Ok(rel) = path.strip_prefix(base) {
                let rel_str = format!("{}/", rel.to_string_lossy().replace('\\', "/"));
                if ignore.is_ignored(&rel_str) {
                    continue;
                }
            }
            collect_files(base, &path, parsers, ignore, out);
        } else if path.is_file() {
            // Skip sensitive files (credentials, secrets, .env)
            if security::is_sensitive_file(&path) {
                continue;
            }
            // Check .voidignore for files
            if let Ok(rel) = path.strip_prefix(base)
                && ignore.is_ignored(&rel.to_string_lossy())
            {
                continue;
            }
            let ext = path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let matches = parsers
                .iter()
                .any(|p| p.file_extensions().iter().any(|pe| ext == *pe));
            if matches {
                let abs = path.to_string_lossy().replace('\\', "/");
                let base_str = base.to_string_lossy().replace('\\', "/");
                let rel = abs
                    .strip_prefix(&base_str)
                    .unwrap_or(&abs)
                    .trim_start_matches('/')
                    .to_string();
                out.push((abs, rel));
            }
        }
    }
}

/// Try to resolve an import path to a known project module.
fn resolve_import(
    import_path: &str,
    from_file: &str,
    is_relative: bool,
    known_modules: &std::collections::HashSet<String>,
    lang: Language,
) -> Option<String> {
    if is_relative {
        // Resolve relative to current file's directory
        let dir = from_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let cleaned = import_path.trim_start_matches("./");

        let candidates = match lang {
            Language::Python => {
                let dot_path = cleaned.replace('.', "/");
                vec![
                    format!("{}/{}.py", dir, dot_path),
                    format!("{}/{}/__init__.py", dir, dot_path),
                    format!("{}.py", dot_path),
                    format!("{}/__init__.py", dot_path),
                ]
            }
            Language::Go => {
                vec![format!("{}/{}", dir, cleaned), cleaned.to_string()]
            }
            Language::Dart => {
                vec![format!("{}/{}", dir, cleaned), cleaned.to_string()]
            }
            Language::Rust => {
                vec![
                    cleaned.to_string(),
                    format!("{}/{}", dir, cleaned),
                    format!("{}.rs", cleaned.trim_end_matches(".rs")),
                    format!("{}/mod.rs", cleaned.trim_end_matches("/mod.rs")),
                ]
            }
            _ => {
                vec![
                    format!("{}/{}", dir, cleaned),
                    format!("{}/{}.js", dir, cleaned),
                    format!("{}/{}.ts", dir, cleaned),
                    format!("{}/{}/index.js", dir, cleaned),
                    format!("{}/{}/index.ts", dir, cleaned),
                ]
            }
        };

        for candidate in &candidates {
            let normalized = candidate.trim_start_matches('/').to_string();
            if known_modules.contains(&normalized) {
                return Some(normalized);
            }
        }
        return None;
    }

    // Non-relative: try as project module
    match lang {
        Language::Python => {
            let module_path = import_path.replace('.', "/");
            let candidates = vec![
                format!("{}.py", module_path),
                format!("{}/__init__.py", module_path),
                format!("src/{}.py", module_path),
            ];
            for c in &candidates {
                if known_modules.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        Language::Go => {
            // Go external imports contain dots (github.com/...)
            // Internal packages might be resolved by directory
            let last = import_path.rsplit('/').next().unwrap_or(import_path);
            let candidates = vec![
                format!("{}.go", last),
                format!("{}/{}.go", last, last),
                format!("pkg/{}.go", last),
                format!("internal/{}.go", last),
            ];
            for c in &candidates {
                if known_modules.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        Language::Rust => {
            // crate:: paths are handled as relative; external crates won't resolve
            let path = import_path.replace("::", "/");
            let candidates = vec![
                format!("{}.rs", path),
                format!("{}/mod.rs", path),
                format!("src/{}.rs", path),
                format!("src/{}/mod.rs", path),
            ];
            for c in &candidates {
                if known_modules.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        Language::Dart => {
            // package: imports normalized to lib/
            let candidates = vec![
                import_path.to_string(),
                format!("{}.dart", import_path.trim_end_matches(".dart")),
            ];
            for c in &candidates {
                if known_modules.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        _ => {
            // JS/TS: non-relative without ./ is external
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language_python_requirements() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Python));
    }

    #[test]
    fn test_detect_language_python_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname=\"x\"\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Python));
    }

    #[test]
    fn test_detect_language_python_setup_py() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("setup.py"),
            "from setuptools import setup\n",
        )
        .unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Python));
    }

    #[test]
    fn test_detect_language_python_by_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.py"), "print('hello')\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Python));
    }

    #[test]
    fn test_detect_language_javascript() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{\"name\":\"x\"}").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::JavaScript));
    }

    #[test]
    fn test_detect_language_typescript() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{\"name\":\"x\"}").unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::TypeScript));
    }

    #[test]
    fn test_detect_language_go() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/x\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Go));
    }

    #[test]
    fn test_detect_language_dart() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Dart));
    }

    #[test]
    fn test_detect_language_rust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Rust));
    }

    #[test]
    fn test_detect_language_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_language(dir.path()), None);
    }

    #[test]
    fn test_resolve_import_relative_python() {
        let mut known = std::collections::HashSet::new();
        known.insert("services/auth.py".to_string());
        let result = resolve_import("auth", "services/api.py", true, &known, Language::Python);
        assert_eq!(result, Some("services/auth.py".to_string()));
    }

    #[test]
    fn test_resolve_import_relative_js() {
        let mut known = std::collections::HashSet::new();
        known.insert("src/utils.js".to_string());
        let result = resolve_import("./utils", "src/app.js", true, &known, Language::JavaScript);
        assert_eq!(result, Some("src/utils.js".to_string()));
    }

    #[test]
    fn test_resolve_import_nonrelative_python() {
        let mut known = std::collections::HashSet::new();
        known.insert("models.py".to_string());
        let result = resolve_import("models", "app.py", false, &known, Language::Python);
        assert_eq!(result, Some("models.py".to_string()));
    }

    #[test]
    fn test_resolve_import_external() {
        let known = std::collections::HashSet::new();
        let result = resolve_import("flask", "app.py", false, &known, Language::Python);
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_import_rust_crate() {
        let mut known = std::collections::HashSet::new();
        known.insert("src/models.rs".to_string());
        let result = resolve_import("src::models", "src/main.rs", false, &known, Language::Rust);
        assert_eq!(result, Some("src/models.rs".to_string()));
    }

    #[test]
    fn test_resolve_import_go_internal() {
        let mut known = std::collections::HashSet::new();
        known.insert("internal/utils.go".to_string());
        let result = resolve_import(
            "github.com/user/proj/utils",
            "main.go",
            false,
            &known,
            Language::Go,
        );
        assert_eq!(result, Some("internal/utils.go".to_string()));
    }

    #[test]
    fn test_build_graph_python() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from flask import Flask\nimport os\n\ndef main():\n    pass\n",
        )
        .unwrap();
        let graph = build_graph(dir.path());
        assert!(graph.is_some());
        let g = graph.unwrap();
        assert_eq!(g.primary_language, Language::Python);
        assert!(!g.modules.is_empty());
    }

    #[test]
    fn test_build_graph_no_language() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_graph(dir.path()).is_none());
    }

    #[test]
    fn test_build_graph_skips_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();
        std::fs::write(dir.path().join("app.py"), "def main(): pass\n").unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(dir.path().join("node_modules/bad.py"), "x = 1\n").unwrap();
        let graph = build_graph(dir.path()).unwrap();
        assert!(
            !graph
                .modules
                .iter()
                .any(|m| m.path.contains("node_modules"))
        );
    }
}
