//! `void env check`: compare the env vars the code actually reads against
//! `.env.example`.
//!
//! Scans source files for real env reads (`process.env.X`, `os.getenv`,
//! `env::var`, `os.Getenv`, `Platform.environment`, ...) and reports two
//! drift sets: used-but-undocumented and documented-but-dead. `--write`
//! regenerates `.env.example` preserving the existing file's comments and
//! ordering, appending missing names with an empty placeholder — real
//! values NEVER travel: the local `.env` is never even read.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use crate::detector::env::parse_env_keys;

/// Ambient OS vars that virtually every runtime touches — comparing them
/// against .env.example would be pure noise.
const AMBIENT: [&str; 14] = [
    "PATH",
    "HOME",
    "TERM",
    "USER",
    "SHELL",
    "LANG",
    "LC_ALL",
    "PWD",
    "TMPDIR",
    "TEMP",
    "TMP",
    "HOSTNAME",
    "EDITOR",
    "XDG_CONFIG_HOME",
];

const SKIP_DIRS: [&str; 12] = [
    "node_modules",
    ".git",
    "target",
    "build",
    "dist",
    ".dart_tool",
    "__pycache__",
    ".next",
    "vendor",
    ".venv",
    "venv",
    "coverage",
];

const CODE_EXTS: [&str; 13] = [
    "rs", "py", "js", "ts", "jsx", "tsx", "go", "dart", "java", "kt", "rb", "php", "verse",
];

#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvVarUse {
    pub name: String,
    /// First site seen, `file:line`.
    pub site: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvCheckReport {
    /// Example file found, if any (`.env.example` / `.env.sample`).
    pub example_file: Option<String>,
    /// Distinct env vars the code reads.
    pub used: usize,
    /// Used by the code but missing from the example file.
    pub undocumented: Vec<EnvVarUse>,
    /// Present in the example file but never read by the code.
    pub dead: Vec<String>,
}

fn read_res() -> &'static Vec<Regex> {
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    RES.get_or_init(|| {
        [
            // JS/TS: process.env.X | process.env["X"] | import.meta.env.X
            r"process\.env\.([A-Z][A-Z0-9_]+)",
            r#"process\.env\[["']([A-Z][A-Z0-9_]+)["']\]"#,
            r"import\.meta\.env\.([A-Z][A-Z0-9_]+)",
            r#"Deno\.env\.get\(["']([A-Z][A-Z0-9_]+)["']\)"#,
            // Python: os.environ["X"] | os.environ.get("X") | os.getenv("X")
            r#"os\.environ\[["']([A-Z][A-Z0-9_]+)["']\]"#,
            r#"os\.environ\.get\(\s*["']([A-Z][A-Z0-9_]+)["']"#,
            r#"os\.getenv\(\s*["']([A-Z][A-Z0-9_]+)["']"#,
            // Rust: env::var("X") | env!("X") | option_env!("X")
            r#"env::var(?:_os)?\(\s*"([A-Z][A-Z0-9_]+)""#,
            r#"(?:option_)?env!\(\s*"([A-Z][A-Z0-9_]+)""#,
            // Go: os.Getenv("X") | os.LookupEnv("X")
            r#"os\.(?:Getenv|LookupEnv)\(\s*"([A-Z][A-Z0-9_]+)""#,
            // Dart: Platform.environment['X'] | String.fromEnvironment('X')
            r#"Platform\.environment\[["']([A-Z][A-Z0-9_]+)["']\]"#,
            r#"fromEnvironment\(\s*["']([A-Z][A-Z0-9_]+)["']"#,
            // Java/Kotlin: System.getenv("X")
            r#"System\.getenv\(\s*"([A-Z][A-Z0-9_]+)""#,
            // Ruby: ENV["X"] | ENV.fetch("X")
            r#"ENV(?:\.fetch\(|\[)\s*["']([A-Z][A-Z0-9_]+)["']"#,
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

/// Scan the tree for env reads: distinct name → first `file:line` site.
pub fn scan_env_reads(root: &Path) -> HashMap<String, String> {
    let ignore = crate::ignore::VoidIgnore::load(root);
    let mut found: HashMap<String, String> = HashMap::new();
    walk(root, root, &ignore, &mut found, 0);
    found.retain(|name, _| !AMBIENT.contains(&name.as_str()));
    found
}

fn walk(
    root: &Path,
    dir: &Path,
    ignore: &crate::ignore::VoidIgnore,
    found: &mut HashMap<String, String>,
    depth: u32,
) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                continue;
            }
            walk(root, &path, ignore, found, depth + 1);
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if ignore.is_ignored(&rel) {
            continue;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !CODE_EXTS.contains(&ext.as_str()) {
            continue;
        }
        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 1_048_576 {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (idx, line) in content.lines().enumerate() {
            for re in read_res() {
                for caps in re.captures_iter(line) {
                    let var = caps[1].to_string();
                    found
                        .entry(var)
                        .or_insert_with(|| format!("{}:{}", rel, idx + 1));
                }
            }
        }
    }
}

fn example_path(root: &Path) -> Option<PathBuf> {
    for name in [".env.example", ".env.sample"] {
        let p = root.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Compare code reads vs the example file.
pub fn check_env(root: &Path) -> EnvCheckReport {
    let reads = scan_env_reads(root);
    let example = example_path(root);
    let documented: HashSet<String> = example
        .as_ref()
        .map(|p| parse_env_keys(p))
        .unwrap_or_default();

    let mut undocumented: Vec<EnvVarUse> = reads
        .iter()
        .filter(|(name, _)| !documented.contains(*name))
        .map(|(name, site)| EnvVarUse {
            name: name.clone(),
            site: site.clone(),
        })
        .collect();
    undocumented.sort_by(|a, b| a.name.cmp(&b.name));

    let mut dead: Vec<String> = documented
        .iter()
        .filter(|d| !reads.contains_key(*d))
        .cloned()
        .collect();
    dead.sort();

    EnvCheckReport {
        example_file: example.map(|p| p.file_name().unwrap().to_string_lossy().to_string()),
        used: reads.len(),
        undocumented,
        dead,
    }
}

/// Create/update `.env.example`: the existing file survives verbatim
/// (comments, ordering, placeholders); missing used vars are appended with
/// an empty placeholder. Real values never appear — `.env` is not read.
pub fn write_env_example(root: &Path, report: &EnvCheckReport) -> Result<PathBuf, String> {
    let path = root.join(report.example_file.as_deref().unwrap_or(".env.example"));
    let mut content = if path.exists() {
        std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?
    } else {
        String::new()
    };
    if report.undocumented.is_empty() {
        return Ok(path);
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("\n# Added by `void env check --write` — vars the code reads\n");
    for u in &report.undocumented {
        content.push_str(&format!("# used at {}\n{}=\n", u.site, u.name));
    }
    std::fs::write(&path, content)
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_detects_reads_across_languages() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.ts"),
            "const url = process.env.API_URL;\nconst k = process.env[\"STRIPE_KEY\"];\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            "import os\ndb = os.getenv(\"DATABASE_URL\")\nmode = os.environ.get('APP_MODE')\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            "fn main() { let t = std::env::var(\"AUTH_TOKEN_TTL\").ok(); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("main.go"),
            "package main\nfunc main() { _ = os.Getenv(\"REDIS_ADDR\") }\n",
        )
        .unwrap();
        // Ambient vars are filtered out.
        std::fs::write(
            dir.path().join("noise.rs"),
            "let p = std::env::var(\"PATH\");\n",
        )
        .unwrap();

        let reads = scan_env_reads(dir.path());
        for expected in [
            "API_URL",
            "STRIPE_KEY",
            "DATABASE_URL",
            "APP_MODE",
            "AUTH_TOKEN_TTL",
            "REDIS_ADDR",
        ] {
            assert!(
                reads.contains_key(expected),
                "missing {expected}: {reads:?}"
            );
        }
        assert!(!reads.contains_key("PATH"));
        assert!(reads["API_URL"].starts_with("app.ts:"));
    }

    #[test]
    fn test_check_reports_undocumented_and_dead() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.ts"),
            "const a = process.env.API_URL;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".env.example"),
            "# infra\nAPI_URL=\nOLD_FLAG=\n",
        )
        .unwrap();

        let report = check_env(dir.path());
        assert_eq!(report.example_file.as_deref(), Some(".env.example"));
        assert_eq!(report.used, 1);
        assert!(report.undocumented.is_empty());
        assert_eq!(report.dead, vec!["OLD_FLAG"]);

        // A new read appears → undocumented.
        std::fs::write(
            dir.path().join("db.ts"),
            "const d = process.env.DATABASE_URL;\n",
        )
        .unwrap();
        let report = check_env(dir.path());
        assert_eq!(report.undocumented.len(), 1);
        assert_eq!(report.undocumented[0].name, "DATABASE_URL");
    }

    #[test]
    fn test_write_preserves_comments_and_never_copies_values() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.ts"),
            "const d = process.env.DATABASE_URL;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".env.example"),
            "# Keep this comment\nAPI_URL=https://placeholder.example\n",
        )
        .unwrap();
        // A real .env with a secret that must NEVER leak into the example.
        std::fs::write(
            dir.path().join(".env"),
            "DATABASE_URL=postgres://user:sup3rs3cret@db/prod\n",
        )
        .unwrap();

        let report = check_env(dir.path());
        let path = write_env_example(dir.path(), &report).unwrap();
        let content = std::fs::read_to_string(path).unwrap();

        assert!(content.contains("# Keep this comment"));
        assert!(content.contains("API_URL=https://placeholder.example"));
        assert!(content.contains("DATABASE_URL=\n"), "{content}");
        assert!(!content.contains("sup3rs3cret"));

        // Idempotent: second write adds nothing.
        let report = check_env(dir.path());
        assert!(report.undocumented.is_empty());
    }

    #[test]
    fn test_write_creates_example_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.ts"),
            "const a = process.env.API_URL;\n",
        )
        .unwrap();
        let report = check_env(dir.path());
        assert_eq!(report.example_file, None);
        let path = write_env_example(dir.path(), &report).unwrap();
        assert!(path.ends_with(".env.example"));
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("API_URL=\n"));
    }
}
