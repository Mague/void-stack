//! Import parsing for multiple languages.

mod classifier;
pub mod langs;
mod resolve;

use std::path::Path;

use super::graph::*;
use crate::security;
use langs::{cpp, csharp, dart, gdscript, golang, javascript, python, rust_lang, swift, verse};

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
    /// Symbols this file *provides* to importers, for languages that import
    /// symbols rather than files: C# namespaces, Swift modules (SwiftPM
    /// target folder), GDScript `class_name`s. `build_graph` links an
    /// import to every project file declaring that symbol.
    pub declares: Vec<String>,
    /// Symbols visible to this file without an import: C#'s enclosing
    /// namespace (and its parents), the file's own Swift module. Linked like
    /// imports, but never reported as external dependencies.
    pub implicit_imports: Vec<String>,
    /// Type names this file declares (C# / Swift).
    pub type_names: Vec<String>,
    /// Identifiers the file mentions. When set, a symbol import only links to
    /// declaring files whose types this file actually names — `using Game;`
    /// alone must not make a file depend on all 40 files of `Game`.
    pub identifiers: Option<std::collections::HashSet<String>>,
}

/// Add every identifier-shaped token of `line` (`[A-Za-z_][A-Za-z0-9_]*`).
pub(crate) fn collect_identifiers(line: &str, out: &mut std::collections::HashSet<String>) {
    for w in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if w.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && !out.contains(w) {
            out.insert(w.to_string());
        }
    }
}

/// The declared name after a type keyword: `Player<T> : Mono {` → `Player`.
pub(crate) fn type_name_token(tok: &str) -> Option<String> {
    let name: String = tok
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit())).then_some(name)
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

/// Directories to skip during scanning. Includes Phoenix/Elixir caches
/// (`deps/`, `_build/`) so committed dependency trees don't get walked
/// into and produce noisy / mis-classified import graphs.
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
    "deps",
    "_build",
    // Unreal Engine / UEFN generated dirs (user code in Plugins/ is kept)
    "Intermediate",
    "Saved",
    "Binaries",
    "DerivedDataCache",
];

/// Detect the primary language for a directory.
pub fn detect_language(dir: &Path) -> Option<Language> {
    // Engine / platform markers first: a Unity or Godot repo often carries a
    // requirements.txt or package.json for tooling.
    if crate::config::has_unity_markers(dir) {
        return Some(Language::CSharp);
    }
    if crate::config::has_godot_markers(dir) {
        return Some(Language::GdScript);
    }
    if crate::config::has_swift_markers(dir) {
        return Some(Language::Swift);
    }
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
    // Unreal / UEFN: .uproject/.uplugin manifests or loose top-level .verse
    // sources mark the dir as a Verse project.
    // Unreal C++ projects keep their modules in `Source/`; UEFN islands
    // are Verse. Both parsers run either way (see `parsers_for`).
    if crate::config::has_unreal_markers(dir) {
        return Some(if dir.join("Source").is_dir() {
            Language::Cpp
        } else {
            Language::Verse
        });
    }
    // Monorepos keep their manifests in service subdirs (e.g. glowing-robot:
    // sentinel-search/backend). Fall back to the dominant source language
    // across the tree so the dependency graph builds instead of failing.
    detect_dominant_language(dir)
}

/// Source extensions per language for [`detect_dominant_language`], in
/// tie-break order: on equal counts the earlier language wins (engine
/// scripting languages first, so a Unity project with a few helper `.py`
/// files is still C#).
const DOMINANT_EXTS: &[(Language, &[&str])] = &[
    (Language::Verse, &["verse"]),
    (Language::GdScript, &["gd"]),
    (Language::CSharp, &["cs"]),
    (Language::Swift, &["swift"]),
    (Language::Dart, &["dart"]),
    (Language::Python, &["py"]),
    (Language::TypeScript, &["ts", "tsx"]),
    (Language::JavaScript, &["js", "jsx", "mjs"]),
    (Language::Go, &["go"]),
    (Language::Rust, &["rs"]),
    (Language::Mql, &["mq4", "mq5", "mqh"]),
    (Language::Cpp, &["cpp", "cc", "cxx", "c"]),
];

/// Dependency, build and engine-cache folders skipped while sampling.
const DOMINANT_SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "build",
    "target",
    ".dart_tool",
    "dist",
    ".next",
    "vendor",
    "Pods",
    ".venv",
    "venv",
    "Intermediate",
    "Saved",
    "Binaries",
    "DerivedDataCache",
];

/// Count source files per language across the tree (bounded, skipping heavy
/// build dirs) and return the most common one. Heuristic fallback for
/// monorepos with no root manifest.
fn detect_dominant_language(dir: &Path) -> Option<Language> {
    let mut counts = [0u32; DOMINANT_EXTS.len()];
    let mut stack = vec![dir.to_path_buf()];
    // Stop after this many directory entries: a sample is enough to tell
    // the dominant language, and huge trees would otherwise stall analysis.
    let mut budget: u32 = 40_000;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !DOMINANT_SKIP_DIRS.contains(&name) {
                    stack.push(p);
                }
            } else if let Some(ext) = p.extension().and_then(|e| e.to_str())
                && let Some(i) = DOMINANT_EXTS
                    .iter()
                    .position(|(_, exts)| exts.contains(&ext))
            {
                counts[i] += 1;
            }
        }
        if budget == 0 {
            break;
        }
    }
    let max = counts.iter().copied().max().filter(|m| *m > 0)?;
    DOMINANT_EXTS
        .iter()
        .zip(counts)
        .find(|(_, c)| *c == max)
        .map(|((lang, _), _)| *lang)
}

/// Build a dependency graph for a project directory.
pub fn build_graph(dir: &Path) -> Option<DependencyGraph> {
    let lang = detect_language(dir)?;
    let parsers = parsers_for(lang);

    // Walk the directory (respecting .voidignore)
    let dir_str = dir.to_string_lossy().replace('\\', "/");
    let ignore = crate::ignore::VoidIgnore::load(dir);
    let mut file_paths: Vec<(String, String)> = Vec::new(); // (abs_path, rel_path)
    collect_files(dir, dir, &parsers, &ignore, &mut file_paths);

    let (mut modules, parsed) = parse_files(&file_paths, &parsers);
    let known_modules: std::collections::HashSet<String> =
        file_paths.into_iter().map(|(_, rel)| rel).collect();
    let (edges, external_deps) = resolve::resolve_edges(&parsed, &known_modules);

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

/// Import parsers for a project whose main language is `lang`.
fn parsers_for(lang: Language) -> Vec<Box<dyn ImportParser>> {
    match lang {
        Language::Python => vec![Box::new(python::PythonParser)],
        Language::JavaScript | Language::TypeScript => vec![Box::new(javascript::JsParser)],
        Language::Go => vec![Box::new(golang::GoParser)],
        Language::Dart => vec![Box::new(dart::DartParser)],
        Language::Rust => vec![Box::new(rust_lang::RustParser)],
        // Unreal: Verse (UEFN) and C++ modules can share a project.
        Language::Verse | Language::Cpp => {
            vec![Box::new(verse::VerseParser), Box::new(cpp::CppParser)]
        }
        // Unity is C#; Godot mixes GDScript and C# (.NET builds).
        Language::CSharp | Language::GdScript => vec![
            Box::new(csharp::CSharpParser),
            Box::new(gdscript::GdScriptParser),
        ],
        Language::Swift => vec![Box::new(swift::SwiftParser)],
        Language::Mql => vec![Box::new(cpp::MqlParser)],
    }
}

/// A parsed file: relative path, language, parse result.
type ParsedFile = (String, Language, ParseResult);

/// Parse every file with the first parser that handles its extension.
fn parse_files(
    file_paths: &[(String, String)],
    parsers: &[Box<dyn ImportParser>],
) -> (Vec<ModuleNode>, Vec<ParsedFile>) {
    let mut modules = Vec::new();
    let mut parsed = Vec::new();
    for (abs_path, rel_path) in file_paths {
        let Some(content) = crate::fs_util::read_source(std::path::Path::new(abs_path)) else {
            continue;
        };
        let Some(parser) = parsers.iter().find(|p| {
            p.file_extensions()
                .iter()
                .any(|ext| rel_path.ends_with(ext))
        }) else {
            continue;
        };
        let result = parser.parse_file(&content, rel_path);
        modules.push(ModuleNode {
            path: rel_path.clone(),
            language: parser.language(),
            layer: classifier::classify_layer(rel_path, &content),
            loc: result.loc,
            class_count: result.class_count,
            function_count: result.function_count,
            is_hub: result.is_hub,
            has_framework_macros: result.has_framework_macros,
        });
        parsed.push((rel_path.clone(), parser.language(), result));
    }
    (modules, parsed)
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
            if SKIP_DIRS.contains(&name.as_ref()) || ignore.is_generated_dir(&name) {
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

/// Project paths a relative import may refer to, most specific first.
fn relative_candidates(lang: Language, dir: &str, cleaned: &str) -> Vec<String> {
    match lang {
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

        let candidates = relative_candidates(lang, dir, cleaned);

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
        // `res://` paths (already stripped) are project-root relative.
        Language::GdScript if known_modules.contains(import_path) => {
            return Some(import_path.to_string());
        }

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
    fn test_detect_language_verse_by_uproject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyGame.uproject"), "{}").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Verse));
    }

    #[test]
    fn test_detect_language_verse_by_verse_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("device.verse"),
            "using { /Fortnite.com/Devices }\n",
        )
        .unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Verse));
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

    // ── detect_language precedence ─────────────────────────────────────

    #[test]
    fn test_detect_language_python_beats_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Python));
    }

    #[test]
    fn test_detect_language_package_json_beats_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::JavaScript));
    }

    #[test]
    fn test_detect_language_go_beats_cargo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x\n").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Go));
    }

    #[test]
    fn test_detect_language_pubspec_beats_cargo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app\n").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Dart));
    }

    // ── detect_dominant_language fallback (monorepos, no root manifest) ─

    #[test]
    fn test_detect_dominant_language_monorepo_python() {
        // No root manifest and no top-level .py files: the dominant source
        // language across subdirectories must win.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("backend")).unwrap();
        std::fs::write(dir.path().join("backend/a.py"), "x = 1\n").unwrap();
        std::fs::write(dir.path().join("backend/b.py"), "y = 2\n").unwrap();
        std::fs::create_dir(dir.path().join("frontend")).unwrap();
        std::fs::write(dir.path().join("frontend/x.js"), "const a = 1;\n").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Python));
    }

    #[test]
    fn test_detect_dominant_language_typescript() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("web")).unwrap();
        std::fs::write(dir.path().join("web/app.tsx"), "").unwrap();
        std::fs::write(dir.path().join("web/util.ts"), "").unwrap();
        std::fs::create_dir(dir.path().join("api")).unwrap();
        std::fs::write(dir.path().join("api/main.go"), "").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::TypeScript));
    }

    #[test]
    fn test_detect_dominant_language_go() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("svc")).unwrap();
        std::fs::write(dir.path().join("svc/main.go"), "").unwrap();
        std::fs::write(dir.path().join("svc/util.go"), "").unwrap();
        std::fs::create_dir(dir.path().join("web")).unwrap();
        std::fs::write(dir.path().join("web/a.js"), "").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Go));
    }

    #[test]
    fn test_detect_dominant_language_rust_in_subdir() {
        // Rust sources in a subdir but no root Cargo.toml.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("core")).unwrap();
        std::fs::write(dir.path().join("core/lib.rs"), "").unwrap();
        std::fs::write(dir.path().join("core/main.rs"), "").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Rust));
    }

    #[test]
    fn test_detect_dominant_language_verse_wins_ties() {
        // Verse is checked first on a tie, and .verse files in subdirs do
        // not trigger the top-level Unreal-marker check.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("game")).unwrap();
        std::fs::write(dir.path().join("game/device.verse"), "").unwrap();
        std::fs::create_dir(dir.path().join("app")).unwrap();
        std::fs::write(dir.path().join("app/x.dart"), "").unwrap();
        assert_eq!(detect_language(dir.path()), Some(Language::Verse));
    }

    #[test]
    fn test_detect_dominant_language_skips_build_dirs() {
        // Sources living only in node_modules/target must not count.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(dir.path().join("node_modules/x.js"), "").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/y.rs"), "").unwrap();
        assert_eq!(detect_language(dir.path()), None);
    }

    // ── build_graph per language ────────────────────────────────────────

    #[test]
    fn test_build_graph_typescript_internal_and_external_edges() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/app.ts"),
            "import { util } from \"./utils\";\nimport React from \"react\";\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/utils.ts"), "export const util = 1;\n").unwrap();

        let g = build_graph(dir.path()).unwrap();
        assert_eq!(g.primary_language, Language::TypeScript);
        assert_eq!(g.modules.len(), 2, "{:?}", g.modules.len());

        // Relative import resolved to the project module, not external.
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == "src/app.ts" && e.to == "src/utils.ts" && !e.is_external),
            "relative import must resolve internally"
        );
        // Bare package import is external.
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == "src/app.ts" && e.to == "react" && e.is_external),
            "package import must be external"
        );
        assert!(g.external_deps.contains("react"));
    }

    #[test]
    fn test_build_graph_go_external_deps() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/x\n").unwrap();
        std::fs::write(
            dir.path().join("main.go"),
            "package main\n\nimport (\n\t\"fmt\"\n\t\"github.com/user/dep\"\n)\n\nfunc main() {}\n",
        )
        .unwrap();

        let g = build_graph(dir.path()).unwrap();
        assert_eq!(g.primary_language, Language::Go);
        assert_eq!(g.modules.len(), 1);
        assert!(g.external_deps.contains("fmt"), "{:?}", g.external_deps);
        assert!(
            g.external_deps.contains("github.com"),
            "external package name is the first path segment: {:?}",
            g.external_deps
        );
    }

    #[test]
    fn test_build_graph_rust_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/main.rs"),
            "use std::collections::HashMap;\n\nfn main() {\n    let _m: HashMap<u8, u8> = HashMap::new();\n}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/utils.rs"), "pub fn helper() {}\n").unwrap();

        let g = build_graph(dir.path()).unwrap();
        assert_eq!(g.primary_language, Language::Rust);
        assert_eq!(g.modules.len(), 2);
        assert!(g.modules.iter().all(|m| m.language == Language::Rust));
    }

    #[test]
    fn test_build_graph_verse_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyGame.uproject"), "{}").unwrap();
        std::fs::write(
            dir.path().join("device.verse"),
            "using { /Fortnite.com/Devices }\nusing { Helpers }\n\nmy_device := class(creative_device):\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("helpers.verse"), "Helpers := module:\n").unwrap();

        let g = build_graph(dir.path()).unwrap();
        assert_eq!(g.primary_language, Language::Verse);
        assert_eq!(g.modules.len(), 2);
        assert!(g.modules.iter().all(|m| m.language == Language::Verse));
        // Absolute digest paths must produce external edges.
        assert!(
            g.edges
                .iter()
                .any(|e| e.to.contains("/Fortnite.com/Devices") && e.is_external),
            "digest using must be an external edge"
        );
    }

    #[test]
    fn test_build_graph_skips_unreal_generated_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyGame.uproject"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("Content")).unwrap();
        std::fs::write(dir.path().join("Content/device.verse"), "").unwrap();
        std::fs::create_dir(dir.path().join("Intermediate")).unwrap();
        std::fs::write(dir.path().join("Intermediate/gen.verse"), "").unwrap();
        std::fs::create_dir(dir.path().join("Saved")).unwrap();
        std::fs::write(dir.path().join("Saved/tmp.verse"), "").unwrap();

        let g = build_graph(dir.path()).unwrap();
        assert!(
            g.modules.iter().any(|m| m.path == "Content/device.verse"),
            "user content must be kept"
        );
        assert!(
            !g.modules
                .iter()
                .any(|m| m.path.contains("Intermediate") || m.path.contains("Saved")),
            "Unreal generated dirs must be skipped"
        );
    }

    #[test]
    fn test_build_graph_respects_voidignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();
        std::fs::write(dir.path().join(".voidignore"), "ignored/\n").unwrap();
        std::fs::write(dir.path().join("app.py"), "def main(): pass\n").unwrap();
        std::fs::create_dir(dir.path().join("ignored")).unwrap();
        std::fs::write(dir.path().join("ignored/skip.py"), "x = 1\n").unwrap();

        let g = build_graph(dir.path()).unwrap();
        assert!(g.modules.iter().any(|m| m.path == "app.py"));
        assert!(
            !g.modules.iter().any(|m| m.path.contains("ignored")),
            ".voidignore patterns must exclude directories"
        );
    }

    // ── resolve_import per-language branches ────────────────────────────

    #[test]
    fn test_resolve_import_relative_go_dir() {
        let mut known = std::collections::HashSet::new();
        known.insert("cmd/helpers.go".to_string());
        let result = resolve_import("helpers.go", "cmd/main.go", true, &known, Language::Go);
        assert_eq!(result, Some("cmd/helpers.go".to_string()));
    }

    #[test]
    fn test_resolve_import_relative_dart() {
        let mut known = std::collections::HashSet::new();
        known.insert("lib/src/models.dart".to_string());
        let result = resolve_import(
            "src/models.dart",
            "lib/main.dart",
            true,
            &known,
            Language::Dart,
        );
        assert_eq!(result, Some("lib/src/models.dart".to_string()));
    }

    #[test]
    fn test_resolve_import_relative_rust() {
        let mut known = std::collections::HashSet::new();
        known.insert("src/utils.rs".to_string());
        let result = resolve_import("utils.rs", "src/main.rs", true, &known, Language::Rust);
        assert_eq!(result, Some("src/utils.rs".to_string()));
    }

    #[test]
    fn test_resolve_import_relative_python_package_init() {
        let mut known = std::collections::HashSet::new();
        known.insert("services/pkg/__init__.py".to_string());
        let result = resolve_import("pkg", "services/api.py", true, &known, Language::Python);
        assert_eq!(result, Some("services/pkg/__init__.py".to_string()));
    }

    #[test]
    fn test_resolve_import_relative_unresolved_returns_none() {
        let known = std::collections::HashSet::new();
        let result = resolve_import("./ghost", "src/app.ts", true, &known, Language::TypeScript);
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_import_ts_nonrelative_is_external() {
        let mut known = std::collections::HashSet::new();
        known.insert("src/utils.ts".to_string());
        let result = resolve_import("lodash", "src/app.ts", false, &known, Language::TypeScript);
        assert_eq!(result, None, "bare JS/TS imports are always external");
    }

    #[test]
    fn test_resolve_import_rust_nonrelative_src_mod_rs() {
        let mut known = std::collections::HashSet::new();
        known.insert("src/utils/mod.rs".to_string());
        let result = resolve_import("utils", "src/main.rs", false, &known, Language::Rust);
        assert_eq!(result, Some("src/utils/mod.rs".to_string()));
    }

    #[test]
    fn test_resolve_import_go_nonrelative_pkg_dir() {
        let mut known = std::collections::HashSet::new();
        known.insert("pkg/db.go".to_string());
        let result = resolve_import(
            "example.com/proj/db",
            "main.go",
            false,
            &known,
            Language::Go,
        );
        assert_eq!(result, Some("pkg/db.go".to_string()));
    }

    #[test]
    fn test_resolve_import_dart_nonrelative() {
        let mut known = std::collections::HashSet::new();
        known.insert("lib/models.dart".to_string());
        let result = resolve_import("lib/models", "lib/main.dart", false, &known, Language::Dart);
        assert_eq!(result, Some("lib/models.dart".to_string()));
    }

    // ── Engines: Unity, Godot, Swift, Unreal C++ ────────────

    fn write(root: &std::path::Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn has_edge(g: &DependencyGraph, from: &str, to: &str) -> bool {
        g.edges
            .iter()
            .any(|e| e.from == from && e.to == to && !e.is_external)
    }

    #[test]
    fn test_unity_graph_links_namespaces_and_skips_library() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "ProjectSettings/ProjectVersion.txt",
            "m_EditorVersion: 6000.0\n",
        );
        write(
            root,
            "Assets/Scripts/Player.cs",
            "using UnityEngine;\nusing Game.Core;\nnamespace Game.Player {\n  public class Player : MonoBehaviour {\n    Health hp;\n    Inventory bag;\n    void Start() {}\n  }\n}\n",
        );
        write(
            root,
            "Assets/Scripts/Core/Health.cs",
            "namespace Game.Core {\n  public class Health {}\n}\n",
        );
        // Same namespace as `using Game.Core`, but Player never names it.
        write(
            root,
            "Assets/Scripts/Core/Stats.cs",
            "namespace Game.Core {\n  public sealed class Stats<T> {}\n}\n",
        );
        // Same namespace as Player: visible without a `using`.
        write(
            root,
            "Assets/Scripts/Inventory.cs",
            "namespace Game.Player {\n  public class Inventory {}\n}\n",
        );
        write(
            root,
            "Library/PackageCache/com.unity.x/Gen.cs",
            "namespace Game.Core {}\n",
        );
        write(root, "requirements.txt", "");

        assert_eq!(detect_language(root), Some(Language::CSharp));
        let g = build_graph(root).expect("unity graph");
        assert!(
            has_edge(
                &g,
                "Assets/Scripts/Player.cs",
                "Assets/Scripts/Core/Health.cs"
            ),
            "{:?}",
            g.edges.iter().map(|e| (&e.from, &e.to)).collect::<Vec<_>>()
        );
        assert!(
            has_edge(
                &g,
                "Assets/Scripts/Player.cs",
                "Assets/Scripts/Inventory.cs"
            ),
            "same-namespace types need no using"
        );
        assert!(
            !has_edge(
                &g,
                "Assets/Scripts/Player.cs",
                "Assets/Scripts/Core/Stats.cs"
            ),
            "`using Game.Core` alone doesn't depend on every file of it"
        );
        assert!(!has_edge(
            &g,
            "Assets/Scripts/Inventory.cs",
            "Assets/Scripts/Player.cs"
        ));
        assert!(g.external_deps.contains("UnityEngine"));
        assert!(
            g.modules.iter().all(|m| !m.path.starts_with("Library/")),
            "Library/ is Unity's cache, not project code"
        );
    }

    #[test]
    fn test_godot_graph_resolves_res_paths_and_class_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "project.godot", "config_version=5\n");
        write(
            root,
            "player.gd",
            "extends \"res://actors/base.gd\"\nconst Bullet = preload(\"res://weapons/bullet.gd\")\nfunc _ready():\n\tpass\n",
        );
        write(root, "actors/base.gd", "extends CharacterBody2D\n");
        write(root, "weapons/bullet.gd", "extends Area2D\n");
        write(root, "enemy.gd", "extends Node2D\nclass_name Enemy\n");
        write(root, "boss.gd", "extends Enemy\n");
        write(root, ".godot/imported/cache.gd", "extends Node\n");

        assert_eq!(detect_language(root), Some(Language::GdScript));
        let g = build_graph(root).expect("godot graph");
        assert!(has_edge(&g, "player.gd", "actors/base.gd"));
        assert!(has_edge(&g, "player.gd", "weapons/bullet.gd"));
        assert!(has_edge(&g, "boss.gd", "enemy.gd"), "extends a class_name");
        assert!(
            !g.external_deps.contains("CharacterBody2D"),
            "engine classes are not packages: {:?}",
            g.external_deps
        );
        assert!(g.modules.iter().all(|m| !m.path.starts_with(".godot/")));
    }

    #[test]
    fn test_swift_package_graph_links_modules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Package.swift", "// swift-tools-version:5.9\n");
        write(
            root,
            "Sources/GameKit/Engine.swift",
            "public struct Engine {}\n",
        );
        write(
            root,
            "Sources/GameApp/main.swift",
            "import Foundation\nimport GameKit\nlet e = Engine()\n",
        );
        write(
            root,
            "Sources/GameKit/Unused.swift",
            "public final class Unused {}\n",
        );
        write(root, "Sources/GameApp/Hud.swift", "struct Hud {}\n");
        write(root, "Sources/GameApp/App.swift", "let hud = Hud()\n");
        write(
            root,
            ".build/checkouts/dep/Sources/Dep/Dep.swift",
            "import GameKit\n",
        );

        assert_eq!(detect_language(root), Some(Language::Swift));
        let g = build_graph(root).expect("swift graph");
        assert!(has_edge(
            &g,
            "Sources/GameApp/main.swift",
            "Sources/GameKit/Engine.swift"
        ));
        assert!(!has_edge(
            &g,
            "Sources/GameApp/main.swift",
            "Sources/GameKit/Unused.swift"
        ));
        assert!(
            has_edge(&g, "Sources/GameApp/App.swift", "Sources/GameApp/Hud.swift"),
            "files of one module see each other without import"
        );
        assert!(g.external_deps.contains("Foundation"));
        assert!(g.modules.iter().all(|m| !m.path.starts_with(".build/")));
    }

    #[test]
    fn test_unreal_cpp_graph_resolves_module_includes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "MyGame.uproject", "{}\n");
        write(
            root,
            "Source/MyGame/Public/Components/HealthComponent.h",
            "#pragma once\n#include \"CoreMinimal.h\"\n",
        );
        write(
            root,
            "Source/MyGame/Private/MyActor.cpp",
            "#include \"MyActor.h\"\n#include \"Components/HealthComponent.h\"\n#include <vector>\nvoid AMyActor::BeginPlay()\n{\n}\n",
        );
        write(root, "Source/MyGame/Private/MyActor.h", "#pragma once\n");
        write(
            root,
            "Intermediate/Build/Gen.cpp",
            "#include \"MyActor.h\"\n",
        );

        assert_eq!(detect_language(root), Some(Language::Cpp));
        let g = build_graph(root).expect("unreal graph");
        assert!(has_edge(
            &g,
            "Source/MyGame/Private/MyActor.cpp",
            "Source/MyGame/Public/Components/HealthComponent.h"
        ));
        assert!(has_edge(
            &g,
            "Source/MyGame/Private/MyActor.cpp",
            "Source/MyGame/Private/MyActor.h"
        ));
        assert!(g.external_deps.contains("CoreMinimal.h"));
        assert!(g.external_deps.contains("vector"));
        assert!(
            g.modules
                .iter()
                .all(|m| !m.path.starts_with("Intermediate/"))
        );
    }

    #[test]
    fn test_mql_project_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "Experts/Bot.mq4",
            "#property strict\n#include <Trade\\Risk.mqh>\n#include \"Signals.mqh\"\n#include <stdlib.mqh>\nvoid OnTick()\n{\n}\n",
        );
        write(
            root,
            "Experts/Signals.mqh",
            "bool IsNewBar()\n{\n   return(true);\n}\n",
        );
        write(
            root,
            "Include/Trade/Risk.mqh",
            "double Lots()\n{\n   return(0.1);\n}\n",
        );

        assert_eq!(detect_language(root), Some(Language::Mql));
        let g = build_graph(root).expect("mql graph");
        assert!(has_edge(&g, "Experts/Bot.mq4", "Experts/Signals.mqh"));
        assert!(has_edge(&g, "Experts/Bot.mq4", "Include/Trade/Risk.mqh"));
        assert!(g.external_deps.contains("stdlib.mqh"));
    }
}
