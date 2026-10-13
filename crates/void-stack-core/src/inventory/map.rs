//! `project_map`: the whole shape of a project in a few kilobytes — its
//! packages, where execution starts, how the code is spread across
//! directories and what the key documents say.
//!
//! This is what an agent cannot get cheaply on its own: listing files
//! level by level costs tens of calls, and searching never tells you what
//! you did *not* find (a crate nobody searched for stays invisible).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use super::walk::{SourceFile, language, walk};

/// A package / workspace member found by its manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// Directory relative to the root ("" for the root itself).
    pub dir: String,
    pub name: String,
    /// "Rust bin", "Rust lib", "Node · next", "Flutter app", "Go", …
    pub kind: String,
    pub description: String,
    /// Code files under `dir` (nested packages included).
    pub code_files: usize,
}

const MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pubspec.yaml",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "mix.exs",
    "composer.json",
    "build.gradle",
    "build.gradle.kts",
    "pom.xml",
    "Package.swift",
];

/// Read a package manifest. `None` when it is not a package (a Cargo
/// workspace root without `[package]`, a package.json with no name…).
fn parse_manifest(root: &Path, rel: &str) -> Option<(String, String, String)> {
    let path = root.join(rel);
    let file = rel.rsplit('/').next().unwrap_or(rel);
    let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let dir_name = dir
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("(root)");
    let text = std::fs::read_to_string(&path).ok()?;
    let exists = |p: &str| root.join(dir).join(p).exists();
    match file {
        "Cargo.toml" => {
            let v: toml::Value = toml::from_str(&text).ok()?;
            let pkg = v.get("package")?;
            let name = pkg.get("name")?.as_str()?.to_string();
            let desc = pkg
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let bins: Vec<String> = v
                .get("bin")
                .and_then(|b| b.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|b| b.get("name").and_then(|n| n.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let kind = if !bins.is_empty() {
                format!("Rust bin `{}`", bins.join("`, `"))
            } else if exists("src/main.rs") {
                "Rust bin".to_string()
            } else {
                "Rust lib".to_string()
            };
            Some((name, kind, desc))
        }
        "package.json" => {
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or(dir_name)
                .to_string();
            let desc = v
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let deps = |k: &str| {
                v.get("dependencies").and_then(|d| d.get(k)).is_some()
                    || v.get("devDependencies").and_then(|d| d.get(k)).is_some()
            };
            let framework = [
                ("next", "next"),
                ("@tauri-apps/api", "tauri"),
                ("electron", "electron"),
                ("@nestjs/core", "nest"),
                ("express", "express"),
                ("fastify", "fastify"),
                ("hono", "hono"),
                ("react-native", "react-native"),
                ("expo", "expo"),
                ("nuxt", "nuxt"),
                ("vue", "vue"),
                ("svelte", "svelte"),
                ("react", "react"),
                ("@modelcontextprotocol/sdk", "mcp"),
            ]
            .iter()
            .find(|(dep, _)| deps(dep))
            .map(|(_, f)| *f);
            let kind = match (framework, v.get("bin").is_some()) {
                (Some(f), _) => format!("Node · {f}"),
                (None, true) => "Node cli".to_string(),
                (None, false) => "Node".to_string(),
            };
            Some((name, kind, desc))
        }
        "pubspec.yaml" => {
            let v: serde_yaml::Value = serde_yaml::from_str(&text).ok()?;
            let name = v.get("name")?.as_str()?.to_string();
            let desc = v
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let flutter = v
                .get("dependencies")
                .and_then(|d| d.get("flutter"))
                .is_some();
            let app = exists("lib/main.dart");
            let kind = match (flutter, app) {
                (true, true) => "Flutter app",
                (true, false) => "Flutter package",
                (false, true) => "Dart app",
                (false, false) => "Dart package",
            };
            Some((name, kind.to_string(), desc))
        }
        "go.mod" => {
            let module = text
                .lines()
                .find_map(|l| l.trim().strip_prefix("module "))?
                .trim()
                .to_string();
            Some((module, "Go module".to_string(), String::new()))
        }
        "pyproject.toml" => {
            let v: toml::Value = toml::from_str(&text).ok()?;
            let proj = v
                .get("project")
                .or_else(|| v.get("tool").and_then(|t| t.get("poetry")))?;
            let name = proj.get("name")?.as_str()?.to_string();
            let desc = proj
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let scripts = proj
                .get("scripts")
                .and_then(|s| s.as_table())
                .map(|t| t.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            let kind = if scripts.is_empty() {
                "Python".to_string()
            } else {
                format!("Python cli `{}`", scripts.join("`, `"))
            };
            Some((name, kind, desc))
        }
        "setup.py" => {
            let name = regex::Regex::new(r#"name\s*=\s*["']([^"']+)"#)
                .ok()?
                .captures(&text)
                .map(|c| c[1].to_string())
                .unwrap_or_else(|| dir_name.to_string());
            Some((name, "Python".to_string(), String::new()))
        }
        "mix.exs" => {
            let name = regex::Regex::new(r"app:\s*:(\w+)")
                .ok()?
                .captures(&text)
                .map(|c| c[1].to_string())
                .unwrap_or_else(|| dir_name.to_string());
            let kind = if text.contains(":phoenix") {
                "Elixir · phoenix"
            } else {
                "Elixir"
            };
            Some((name, kind.to_string(), String::new()))
        }
        "composer.json" => {
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or(dir_name)
                .to_string();
            let desc = v
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            Some((name, "PHP".to_string(), desc))
        }
        "build.gradle" | "build.gradle.kts" => {
            let kind = if text.contains("com.android.application") {
                "Android app"
            } else if text.contains("com.android.library") {
                "Android library"
            } else {
                "Gradle"
            };
            Some((dir_name.to_string(), kind.to_string(), String::new()))
        }
        "pom.xml" => Some((dir_name.to_string(), "Maven".to_string(), String::new())),
        "Package.swift" => Some((
            dir_name.to_string(),
            "Swift package".to_string(),
            String::new(),
        )),
        _ => None,
    }
}

/// Every package under the root, outermost first.
pub fn packages(root: &Path) -> Vec<Package> {
    packages_from(root, &walk(root))
}

fn packages_from(root: &Path, files: &[SourceFile]) -> Vec<Package> {
    let mut out: Vec<Package> = Vec::new();
    for f in files {
        let file = f.rel.rsplit('/').next().unwrap_or(&f.rel);
        if !MANIFESTS.contains(&file) {
            continue;
        }
        let dir = f
            .rel
            .rsplit_once('/')
            .map(|(d, _)| d)
            .unwrap_or("")
            .to_string();
        // One package per directory (build.gradle next to settings, a
        // pyproject next to setup.py…): the first manifest wins.
        if out.iter().any(|p| p.dir == dir) {
            continue;
        }
        let Some((name, kind, description)) = parse_manifest(root, &f.rel) else {
            continue;
        };
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let code_files = files
            .iter()
            .filter(|c| c.rel.starts_with(&prefix) && language(&c.rel).is_some())
            .count();
        out.push(Package {
            dir,
            name,
            kind,
            description,
            code_files,
        });
    }
    out.sort_by(|a, b| {
        a.dir
            .matches('/')
            .count()
            .cmp(&b.dir.matches('/').count())
            .then(a.dir.cmp(&b.dir))
    });
    out
}

/// Files where execution starts: binaries, servers, app shells.
pub(crate) fn entry_files(files: &[SourceFile]) -> Vec<String> {
    let mut out: Vec<String> = files
        .iter()
        .filter(|f| language(&f.rel).is_some())
        .filter(|f| is_entry_file(&f.rel))
        .map(|f| f.rel.clone())
        .collect();
    out.sort_by(|a, b| {
        a.matches('/')
            .count()
            .cmp(&b.matches('/').count())
            .then(a.cmp(b))
    });
    out
}

fn is_entry_file(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let parent = lower.rsplit('/').nth(1).unwrap_or("");
    if lower.contains("/test") || lower.starts_with("test") || lower.contains("example") {
        return false;
    }
    matches!(
        name,
        "main.rs"
            | "main.go"
            | "main.py"
            | "__main__.py"
            | "manage.py"
            | "main.dart"
            | "main.ts"
            | "main.tsx"
            | "main.js"
            | "main.jsx"
            | "program.cs"
            | "main.swift"
            | "main.kt"
            | "application.kt"
    ) || (matches!(
        name,
        "server.ts"
            | "server.js"
            | "server.py"
            | "app.py"
            | "run.py"
            | "cli.py"
            | "wsgi.py"
            | "asgi.py"
    ) && !lower.contains("/components/"))
        || (matches!(name, "index.ts" | "index.js" | "index.tsx")
            && matches!(parent, "src" | "" | "server" | "api" | "bin"))
        || (parent == "bin" && lower.contains("src/bin/"))
        || (lower.ends_with("app/layout.tsx")
            || lower.ends_with("app/layout.jsx")
            || lower.ends_with("pages/_app.tsx")
            || lower.ends_with("pages/_app.js"))
        || (name == "application.ex" || name == "router.ex" || name == "endpoint.ex")
}

/// Title and first sentence of a markdown document.
fn doc_summary(root: &Path, rel: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join(rel)).ok()?;
    let mut title = None;
    let mut sentence = None;
    let mut in_code = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code
            || t.is_empty()
            || t.starts_with("<")
            || t.starts_with('[')
            || t.starts_with('!')
            || t.starts_with('|')
            || t.starts_with("---")
        {
            continue;
        }
        if let Some(h) = t.strip_prefix('#') {
            if title.is_none() {
                title = Some(h.trim_start_matches('#').trim().to_string());
            }
            continue;
        }
        if t.starts_with('>') && sentence.is_some() {
            continue;
        }
        let body = t.trim_start_matches('>').trim();
        if body.len() < 20 {
            continue;
        }
        let end = body
            .char_indices()
            .find(|(i, c)| *c == '.' && body[i + 1..].starts_with(' '))
            .map(|(i, _)| i + 1)
            .unwrap_or(body.len());
        sentence = Some(clip(&body[..end], 160));
        break;
    }
    match (title, sentence) {
        (Some(t), Some(s)) => Some(format!("{} — {}", clip(&t, 80), s)),
        (Some(t), None) => Some(clip(&t, 80)),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Code files per directory, recursively (key "" = root).
fn dir_counts(files: &[SourceFile]) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for f in files.iter().filter(|f| language(&f.rel).is_some()) {
        let mut dir = f.rel.as_str();
        *counts.entry(String::new()).or_default() += 1;
        while let Some((parent, _)) = dir.rsplit_once('/') {
            *counts.entry(parent.to_string()).or_default() += 1;
            dir = parent;
        }
    }
    counts
}

const TREE_CHILDREN: usize = 8;
const TREE_LINES: usize = 60;

fn render_tree(counts: &HashMap<String, usize>, depth: usize) -> String {
    // children[parent] = [(dir, count)]
    let mut children: HashMap<&str, Vec<(&str, usize)>> = HashMap::new();
    for (dir, n) in counts {
        if dir.is_empty() {
            continue;
        }
        let parent = dir.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        children.entry(parent).or_default().push((dir.as_str(), *n));
    }
    for v in children.values_mut() {
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    }
    let mut out = String::new();
    let mut lines = 0usize;
    fn rec(
        dir: &str,
        level: usize,
        depth: usize,
        children: &HashMap<&str, Vec<(&str, usize)>>,
        out: &mut String,
        lines: &mut usize,
    ) {
        let Some(kids) = children.get(dir) else {
            return;
        };
        for (i, (child, n)) in kids.iter().enumerate() {
            if *lines >= TREE_LINES {
                return;
            }
            if i == TREE_CHILDREN {
                let rest: usize = kids[i..].iter().map(|k| k.1).sum();
                let _ = writeln!(
                    out,
                    "{}… +{} dirs ({} files)",
                    "  ".repeat(level),
                    kids.len() - i,
                    rest
                );
                *lines += 1;
                return;
            }
            // Collapse single-child chains: src/main/java/com/acme/
            let mut shown = child.to_string();
            let mut cur: &str = child;
            while let Some(only) = children.get(cur).filter(|k| k.len() == 1 && k[0].1 == *n) {
                cur = only[0].0;
                shown = cur.to_string();
            }
            let label = if dir.is_empty() {
                shown.clone()
            } else {
                shown
                    .strip_prefix(&format!("{dir}/"))
                    .unwrap_or(&shown)
                    .to_string()
            };
            let _ = writeln!(out, "{}{}/ ({})", "  ".repeat(level), label, n);
            *lines += 1;
            if level + 1 < depth {
                rec(cur, level + 1, depth, children, out, lines);
            }
        }
    }
    rec("", 0, depth, &children, &mut out, &mut lines);
    out
}

/// The project map as compact text (aim: 2–4 KB).
pub fn project_map(name: &str, root: &Path, depth: usize) -> String {
    let files = walk(root);
    let code: Vec<&SourceFile> = files
        .iter()
        .filter(|f| language(&f.rel).is_some())
        .collect();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# {} — {} code files, {} files total",
        name,
        code.len(),
        files.len()
    );

    // Languages
    let mut langs: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &code {
        if let Some(l) = language(&f.rel) {
            *langs.entry(l).or_default() += 1;
        }
    }
    let mut langs: Vec<(&str, usize)> = langs.into_iter().collect();
    langs.sort_by_key(|b| std::cmp::Reverse(b.1));
    let _ = writeln!(
        out,
        "Languages: {}",
        langs
            .iter()
            .take(8)
            .map(|(l, n)| format!("{l} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Packages
    let pkgs = packages_from(root, &files);
    if !pkgs.is_empty() {
        let _ = writeln!(out, "\n## Packages ({})", pkgs.len());
        for p in pkgs.iter().take(40) {
            let dir = if p.dir.is_empty() {
                "."
            } else {
                p.dir.as_str()
            };
            let desc = if p.description.is_empty() {
                String::new()
            } else {
                format!(" — {}", clip(&p.description, 90))
            };
            let _ = writeln!(
                out,
                "- {} · {} · {} · {} files{}",
                dir, p.name, p.kind, p.code_files, desc
            );
        }
        if pkgs.len() > 40 {
            let _ = writeln!(out, "- … +{} more", pkgs.len() - 40);
        }
    }

    // Entry points
    let entries = entry_files(&files);
    if !entries.is_empty() {
        let shown: Vec<&str> = entries.iter().take(20).map(|s| s.as_str()).collect();
        let more = if entries.len() > 20 {
            format!(" … +{}", entries.len() - 20)
        } else {
            String::new()
        };
        let _ = writeln!(out, "\n## Entry points\n{}{}", shown.join(", "), more);
    }

    // Tree
    let counts = dir_counts(&files);
    let _ = writeln!(
        out,
        "\n## Code by directory (files)\n{}",
        render_tree(&counts, depth.max(1))
    );

    // Docs
    let docs: Vec<&SourceFile> = files
        .iter()
        .filter(|f| {
            let l = f.rel.to_ascii_lowercase();
            l.ends_with(".md") && !l.contains("node_modules") && !l.starts_with("changelog")
        })
        .collect();
    let key_docs: Vec<&SourceFile> = docs
        .iter()
        .copied()
        .filter(|f| {
            let depth = f.rel.matches('/').count();
            depth == 0 || (depth == 1 && f.rel.to_ascii_lowercase().starts_with("docs/"))
        })
        .take(12)
        .collect();
    if !key_docs.is_empty() {
        let _ = writeln!(out, "## Docs ({} markdown files)", docs.len());
        for d in key_docs {
            let summary = doc_summary(root, &d.rel).unwrap_or_default();
            let _ = writeln!(
                out,
                "- {}{}",
                d.rel,
                if summary.is_empty() {
                    String::new()
                } else {
                    format!(": {summary}")
                }
            );
        }
    }
    out.push_str(
        "\nNext: list_entrypoints for routes/RPCs/tools/commands; semantic_search for details.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        let files: &[(&str, &str)] = &[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            (
                "crates/app-core/Cargo.toml",
                "[package]\nname = \"app-core\"\ndescription = \"Core library\"\n",
            ),
            ("crates/app-core/src/lib.rs", "pub fn x() {}"),
            ("crates/app-core/src/search/mod.rs", "pub fn s() {}"),
            (
                "crates/app-cli/Cargo.toml",
                "[package]\nname = \"app-cli\"\n[[bin]]\nname = \"app\"\npath = \"src/main.rs\"\n",
            ),
            ("crates/app-cli/src/main.rs", "fn main() {}"),
            (
                "crates/app-gpu/Cargo.toml",
                "[package]\nname = \"app-gpu\"\n",
            ),
            ("crates/app-gpu/src/main.rs", "fn main() {}"),
            (
                "web/package.json",
                "{\"name\":\"web\",\"dependencies\":{\"next\":\"15\",\"react\":\"19\"}}",
            ),
            ("web/app/layout.tsx", "export default function L() {}"),
            ("web/app/page.tsx", "export default function P() {}"),
            (
                "README.md",
                "# App\n\n![badge](x)\n\nApp runs your local dev stack from one place. More text.\n",
            ),
            (
                "docs/arch.md",
                "# Architecture\n\nThe system has three layers and many details.\n",
            ),
        ];
        for (p, c) in files {
            let f = r.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, c).unwrap();
        }
        dir
    }

    #[test]
    fn finds_every_package_including_small_ones() {
        let dir = fixture();
        let pkgs = packages(dir.path());
        let names: Vec<&str> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["web", "app-cli", "app-core", "app-gpu"]);
        let cli = pkgs.iter().find(|p| p.name == "app-cli").unwrap();
        assert_eq!(cli.kind, "Rust bin `app`");
        let gpu = pkgs.iter().find(|p| p.name == "app-gpu").unwrap();
        assert_eq!(gpu.kind, "Rust bin");
        let core = pkgs.iter().find(|p| p.name == "app-core").unwrap();
        assert_eq!((core.kind.as_str(), core.code_files), ("Rust lib", 2));
        assert_eq!(pkgs[0].kind, "Node · next");
    }

    #[test]
    fn map_is_compact_and_complete() {
        let dir = fixture();
        let map = project_map("app", dir.path(), 3);
        assert!(map.len() < 4000, "{}", map.len());
        for needle in [
            "crates/app-gpu · app-gpu",
            "Rust bin `app`",
            "## Entry points",
            "crates/app-cli/src/main.rs",
            "web/app/layout.tsx",
            "README.md: App — App runs your local dev stack from one place.",
            "docs/arch.md: Architecture",
            "crates/ (4)",
            "  app-core/src/ (2)",
        ] {
            assert!(map.contains(needle), "missing {needle:?} in\n{map}");
        }
    }

    #[test]
    fn tree_collapses_single_child_chains() {
        let files: Vec<SourceFile> = [
            "src/main/java/com/acme/App.java",
            "src/main/java/com/acme/Util.java",
        ]
        .iter()
        .map(|r| SourceFile {
            rel: r.to_string(),
            size: 1,
        })
        .collect();
        let tree = render_tree(&dir_counts(&files), 3);
        assert_eq!(tree.trim(), "src/main/java/com/acme/ (2)");
    }
}
