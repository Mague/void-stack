//! Import parsing for multiple languages.

pub mod python;
pub mod javascript;
pub mod golang;
pub mod dart;
pub mod rust_lang;

use std::path::Path;

use super::graph::*;
use crate::security;

/// Result of parsing a single file.
pub struct ParseResult {
    pub imports: Vec<RawImport>,
    pub class_count: usize,
    pub function_count: usize,
    pub loc: usize,
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
    "node_modules", ".venv", "venv", "env", "__pycache__", ".git",
    "target", "build", "dist", ".next", ".nuxt", "coverage", ".tox",
    "vendor", "eggs", ".eggs", ".mypy_cache", ".pytest_cache",
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
        let has_py = entries.flatten().any(|e| {
            e.path().extension().map(|ext| ext == "py").unwrap_or(false)
        });
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

    // Walk the directory
    let dir_str = dir.to_string_lossy().replace('\\', "/");
    let mut file_paths: Vec<(String, String)> = Vec::new(); // (abs_path, rel_path)
    collect_files(dir, dir, &parsers, &mut file_paths);

    // Known project modules (for resolving internal vs external)
    let known_modules: std::collections::HashSet<String> = file_paths
        .iter()
        .map(|(_, rel)| rel.clone())
        .collect();

    for (abs_path, rel_path) in &file_paths {
        let content = match std::fs::read_to_string(abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let parser = parsers.iter().find(|p| {
            p.file_extensions().iter().any(|ext| rel_path.ends_with(ext))
        });
        let parser = match parser {
            Some(p) => p,
            None => continue,
        };

        let result = parser.parse_file(&content, rel_path);
        let layer = classify_layer(rel_path, &content);

        modules.push(ModuleNode {
            path: rel_path.clone(),
            language: parser.language(),
            layer,
            loc: result.loc,
            class_count: result.class_count,
            function_count: result.function_count,
        });

        for imp in &result.imports {
            let resolved = resolve_import(&imp.module_path, rel_path, imp.is_relative, &known_modules, parser.language());
            let is_external = resolved.is_none() && !imp.is_relative;

            if is_external {
                let pkg = imp.module_path.split('/').next()
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
            collect_files(base, &path, parsers, out);
        } else if path.is_file() {
            // Skip sensitive files (credentials, secrets, .env)
            if security::is_sensitive_file(&path) {
                continue;
            }
            let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
            let matches = parsers.iter().any(|p| {
                p.file_extensions().iter().any(|pe| ext == *pe)
            });
            if matches {
                let abs = path.to_string_lossy().replace('\\', "/");
                let base_str = base.to_string_lossy().replace('\\', "/");
                let rel = abs.strip_prefix(&base_str)
                    .unwrap_or(&abs)
                    .trim_start_matches('/')
                    .to_string();
                out.push((abs, rel));
            }
        }
    }
}

/// Classify a module into an architectural layer based on path and content.
///
/// Uses table-driven matching for directory names, filenames, and content
/// heuristics to keep cyclomatic complexity low.
fn classify_layer(path: &str, content: &str) -> ArchLayer {
    // 1. Path-level checks (test, config)
    if let Some(layer) = classify_by_path_keywords(path) {
        return layer;
    }

    // 2. Directory name matching (table-driven)
    let parts: Vec<&str> = path.split('/').collect();
    if let Some(layer) = classify_by_directory(&parts) {
        return layer;
    }

    // 3. Filename matching (table-driven)
    if let Some(layer) = classify_by_filename(parts.last().unwrap_or(&"")) {
        return layer;
    }

    // 4. Content heuristics (table-driven)
    if let Some(layer) = classify_by_content(content) {
        return layer;
    }

    ArchLayer::Unknown
}

// ── Table-driven classification helpers ────────────────────────────────────

/// Directory name → ArchLayer mapping table.
const DIR_RULES: &[(&[&str], ArchLayer)] = &[
    // Controller / API layer
    (&["controllers", "controller", "routes", "routers", "handlers",
      "views", "endpoints", "api"], ArchLayer::Controller),
    // Service / business logic layer (includes Rust-specific modules)
    (&["services", "service", "usecases", "use_cases", "domain", "business", "logic",
      // Rust crate directories that implement core business logic
      "runner", "hooks", "detector", "analyzer", "diagram", "docker",
      "audit", "security", "manager", "ai"], ArchLayer::Service),
    // Repository / data access layer
    (&["repositories", "repository", "repos", "dao", "dal", "data",
      "db", "database", "persistence", "migration", "migrations"], ArchLayer::Repository),
    // Model / type definition layer
    (&["models", "model", "entities", "entity", "schemas", "schema",
      "types", "dto", "dtos", "proto"], ArchLayer::Model),
    // Utility / infrastructure layer
    (&["utils", "util", "helpers", "helper", "common", "shared",
      "lib", "core", "middleware", "process_util"], ArchLayer::Utility),
    // Config layer
    (&["config", "configuration", "settings"], ArchLayer::Config),
];

/// Filename substring → ArchLayer mapping table.
const FILENAME_RULES: &[(&[&str], ArchLayer)] = &[
    (&["controller", "handler", "route", "view", "endpoint"], ArchLayer::Controller),
    (&["service", "usecase", "runner", "detector", "analyzer"], ArchLayer::Service),
    (&["repo", "dao", "database", "migration"], ArchLayer::Repository),
    (&["model", "entity", "schema", "proto"], ArchLayer::Model),
    (&["util", "helper", "common"], ArchLayer::Utility),
];

/// Content pattern → ArchLayer mapping table.
const CONTENT_RULES: &[(&[&str], ArchLayer)] = &[
    // Web framework route decorators (Python, JS, Rust)
    (&["@app.", "@router.", "app.get(", "app.post(", "router.get(",
      "#[get(", "#[post(", "#[put(", "#[delete(", // Rust: actix-web, rocket
      "tauri::command"], ArchLayer::Controller),
    // Rust trait implementations and service patterns
    (&["#[async_trait]", "impl Runner for"], ArchLayer::Service),
    // ORM / database patterns
    (&["#[derive(Queryable", "#[derive(Insertable", "diesel::",
      "sqlx::", "sea_orm::"], ArchLayer::Repository),
    // Rust struct/enum definitions (model-heavy files)
    (&["#[derive(Serialize", "#[derive(Deserialize", "pub struct ",
      "pub enum "], ArchLayer::Model),
];

fn classify_by_path_keywords(path: &str) -> Option<ArchLayer> {
    let lower = path.to_lowercase();

    // Test files
    if lower.contains("test") || lower.contains("spec") || lower.starts_with("tests/") {
        return Some(ArchLayer::Test);
    }

    // Config files by extension pattern
    let config_suffixes = ["config.py", "config.js", "config.ts", "config.rs",
                           "config.toml", "config.yaml", "config.yml"];
    if lower.contains(".env") || config_suffixes.iter().any(|s| lower.ends_with(s)) {
        return Some(ArchLayer::Config);
    }

    // Rust mod.rs — classify by parent directory instead
    if lower.ends_with("mod.rs") {
        return None; // fall through to directory classification
    }

    None
}

fn classify_by_directory(parts: &[&str]) -> Option<ArchLayer> {
    for part in parts {
        let p = part.to_lowercase();
        for (names, layer) in DIR_RULES {
            if names.iter().any(|n| *n == p.as_str()) {
                return Some(*layer);
            }
        }
    }
    None
}

fn classify_by_filename(filename: &str) -> Option<ArchLayer> {
    let fn_lower = filename.to_lowercase();
    for (substrings, layer) in FILENAME_RULES {
        if substrings.iter().any(|s| fn_lower.contains(s)) {
            return Some(*layer);
        }
    }
    None
}

fn classify_by_content(content: &str) -> Option<ArchLayer> {
    for (patterns, layer) in CONTENT_RULES {
        if patterns.iter().any(|p| content.contains(p)) {
            return Some(*layer);
        }
    }
    None
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
                vec![
                    format!("{}/{}", dir, cleaned),
                    cleaned.to_string(),
                ]
            }
            Language::Dart => {
                vec![
                    format!("{}/{}", dir, cleaned),
                    cleaned.to_string(),
                ]
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
