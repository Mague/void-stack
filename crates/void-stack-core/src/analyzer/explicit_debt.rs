//! Detect explicit technical debt markers in source code comments.
//!
//! Scans for: TODO, FIXME, HACK, XXX, OPTIMIZE, BUG, TEMP, WORKAROUND.

use std::path::Path;

/// A single explicit debt marker found in source code.
#[derive(Debug, Clone)]
pub struct ExplicitDebtItem {
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub text: String,
    pub language: String,
}

const DEBT_KEYWORDS: &[&str] = &[
    "TODO",
    "FIXME",
    "HACK",
    "XXX",
    "OPTIMIZE",
    "BUG",
    "TEMP",
    "WORKAROUND",
];

const SKIP_DIRS: &[&str] = &[
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
    ".nuxt",
    ".tox",
    ".eggs",
    ".mypy_cache",
    ".pytest_cache",
    "coverage",
];

const CODE_EXTENSIONS: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("py", "python"),
    ("js", "javascript"),
    ("ts", "typescript"),
    ("jsx", "javascript"),
    ("tsx", "typescript"),
    ("go", "go"),
    ("dart", "dart"),
    ("java", "java"),
    ("kt", "kotlin"),
    ("rb", "ruby"),
    ("php", "php"),
    ("c", "c"),
    ("cpp", "cpp"),
    ("h", "c"),
    ("hpp", "cpp"),
    ("cs", "csharp"),
    ("swift", "swift"),
    ("vue", "vue"),
    ("svelte", "svelte"),
];

/// Scan a project directory for explicit debt markers.
pub fn scan_explicit_debt(root: &Path) -> Vec<ExplicitDebtItem> {
    let ignore = crate::ignore::VoidIgnore::load(root);
    let mut items = Vec::new();
    scan_dir(root, root, &ignore, &mut items, 0);
    items.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    items
}

fn scan_dir(
    root: &Path,
    dir: &Path,
    ignore: &crate::ignore::VoidIgnore,
    items: &mut Vec<ExplicitDebtItem>,
    depth: u32,
) {
    if depth > 8 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if SKIP_DIRS.iter().any(|s| name_str.eq_ignore_ascii_case(s)) {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = format!("{}/", rel.to_string_lossy().replace('\\', "/"));
                if ignore.is_ignored(&rel_str) {
                    continue;
                }
            }
            scan_dir(root, &path, ignore, items, depth + 1);
            continue;
        }
        // Check .voidignore for individual files
        if let Ok(rel) = path.strip_prefix(root)
            && ignore.is_ignored(&rel.to_string_lossy())
        {
            continue;
        }

        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let language = match CODE_EXTENSIONS.iter().find(|(e, _)| *e == ext.as_str()) {
            Some((_, lang)) => *lang,
            None => continue,
        };

        // Size limit 1MB
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > 1_048_576 {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        scan_content(&rel_path, &content, language, items);
    }
}

fn scan_content(file: &str, content: &str, language: &str, items: &mut Vec<ExplicitDebtItem>) {
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Only scan comment lines or inline comments
        if !is_comment_context(trimmed, language) {
            continue;
        }

        // Check for uppercase keywords (exact word boundary)
        let upper = line.to_uppercase();
        for keyword in DEBT_KEYWORDS {
            if let Some(pos) = upper.find(keyword) {
                // Verify word boundary: char before should not be alphanumeric
                if pos > 0 {
                    let prev = upper.as_bytes()[pos - 1];
                    if prev.is_ascii_alphanumeric() || prev == b'_' {
                        continue;
                    }
                }
                // Extract the text after the keyword
                let after_kw = &line[pos + keyword.len()..];
                let text = after_kw
                    .trim_start_matches([':', ' ', '-'])
                    .trim()
                    .to_string();

                items.push(ExplicitDebtItem {
                    file: file.to_string(),
                    line: line_num + 1,
                    kind: keyword.to_string(),
                    text,
                    language: language.to_string(),
                });
                break; // One match per line
            }
        }
    }
}

fn is_comment_context(trimmed: &str, language: &str) -> bool {
    match language {
        "python" | "ruby" => trimmed.starts_with('#') || trimmed.contains("# "),
        "rust" | "go" | "javascript" | "typescript" | "java" | "kotlin" | "c" | "cpp"
        | "csharp" | "swift" | "dart" | "php" => {
            trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
                || trimmed.contains("// ")
                || trimmed.contains("/* ")
        }
        "vue" | "svelte" => {
            trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
                || trimmed.starts_with("<!--")
                || trimmed.contains("// ")
                || trimmed.contains("<!-- ")
        }
        _ => trimmed.starts_with("//") || trimmed.starts_with('#'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_text(content: &str, language: &str) -> Vec<ExplicitDebtItem> {
        let mut items = Vec::new();
        scan_content("test.rs", content, language, &mut items);
        items
    }

    #[test]
    fn test_rust_todo() {
        let items = scan_text("// TODO: implement error handling", "rust");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "TODO");
        assert_eq!(items[0].text, "implement error handling");
        assert_eq!(items[0].line, 1);
    }

    #[test]
    fn test_rust_fixme_and_hack() {
        let items = scan_text(
            "// FIXME: this is broken\n\
             fn foo() {}\n\
             // HACK: temporary workaround",
            "rust",
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "FIXME");
        assert_eq!(items[1].kind, "HACK");
    }

    #[test]
    fn test_python_todo() {
        let items = scan_text("# TODO: refactor this function", "python");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "TODO");
        assert_eq!(items[0].text, "refactor this function");
    }

    #[test]
    fn test_python_fixme() {
        let items = scan_text("# FIXME race condition in async handler", "python");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "FIXME");
    }

    #[test]
    fn test_javascript_todo() {
        let items = scan_text(
            "// TODO: add validation\n/* FIXME: memory leak */",
            "javascript",
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "TODO");
        assert_eq!(items[1].kind, "FIXME");
    }

    #[test]
    fn test_no_match_in_code() {
        let items = scan_text("let todo_list = vec![];", "rust");
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_all_keywords() {
        let content = "// TODO: a\n// FIXME: b\n// HACK: c\n// XXX: d\n// OPTIMIZE: e\n// BUG: f\n// TEMP: g\n// WORKAROUND: h";
        let items = scan_text(content, "rust");
        assert_eq!(items.len(), 8);
        let kinds: Vec<&str> = items.iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "TODO",
                "FIXME",
                "HACK",
                "XXX",
                "OPTIMIZE",
                "BUG",
                "TEMP",
                "WORKAROUND"
            ]
        );
    }

    #[test]
    fn test_case_insensitive_match() {
        // Keywords should match case-insensitively
        let items = scan_text("// todo: lowercase", "rust");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "TODO");
    }

    #[test]
    fn test_filesystem_scan() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            "fn main() {\n    // TODO: add logging\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "# FIXME: handle timeout\ndef run(): pass\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(
            dir.path().join("node_modules/lib.js"),
            "// TODO: should be skipped",
        )
        .unwrap();

        let items = scan_explicit_debt(dir.path());
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.language == "python"));
        assert!(items.iter().any(|i| i.language == "rust"));
        // node_modules should be skipped
        assert!(!items.iter().any(|i| i.file.contains("node_modules")));
    }
}
