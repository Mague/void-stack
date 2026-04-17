//! Audit finding suppression via `.void-audit-ignore` and inline directives.
//!
//! ## File-based suppression (`.void-audit-ignore`)
//!
//! ```ignore
//! # Suppress unwrap/expect findings in the regex-heavy vuln_patterns files
//! unwrap-in-prod   crates/void-stack-core/src/audit/vuln_patterns/**
//!
//! # i18n tables are data, not complex logic
//! CC-HIGH          crates/void-stack-tui/src/i18n.rs
//!
//! # Test fixture secrets aren't real
//! secret-aws-*     crates/**/tests/**
//! ```
//!
//! Format: `<rule_glob>  <path_glob>` — one rule per line, `#` for comments.
//! `rule_glob` matches against `finding.id`; `path_glob` matches against
//! `finding.file_path` using simple `*` / `**` expansion.
//!
//! ## Inline suppression
//!
//! ```ignore
//! // void-audit: ignore-next-line
//! let x = thing.unwrap();
//!
//! // void-audit: ignore-file
//! // (at the top of the file — suppresses all findings for this file)
//! ```

use std::path::Path;

use super::findings::SecurityFinding;

/// A single suppression rule parsed from `.void-audit-ignore`.
#[derive(Debug, Clone)]
struct Rule {
    rule_glob: String,
    path_glob: String,
}

/// Load and parse `.void-audit-ignore` from the project root.
/// Returns an empty vec if the file doesn't exist.
fn load_rules(project_path: &Path) -> Vec<Rule> {
    let ignore_path = project_path.join(".void-audit-ignore");
    let content = match std::fs::read_to_string(&ignore_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let mut parts = trimmed.splitn(2, char::is_whitespace);
            let rule_glob = parts.next()?.trim().to_string();
            let path_glob = parts.next()?.trim().to_string();
            if rule_glob.is_empty() || path_glob.is_empty() {
                return None;
            }
            Some(Rule {
                rule_glob,
                path_glob,
            })
        })
        .collect()
}

/// Check whether `text` matches a simple glob (supports `*` and `**`).
fn glob_match(pattern: &str, text: &str) -> bool {
    // Convert glob to a basic regex-like matcher.
    // `**` → match anything (including /)
    // `*`  → match anything except /
    let mut pat = String::with_capacity(pattern.len() + 10);
    pat.push('^');
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next(); // consume second *
                    // skip optional / after **
                    if chars.peek() == Some(&'/') {
                        chars.next();
                    }
                    pat.push_str(".*");
                } else {
                    pat.push_str("[^/]*");
                }
            }
            '.' | '(' | ')' | '+' | '?' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                pat.push('\\');
                pat.push(c);
            }
            _ => pat.push(c),
        }
    }
    pat.push('$');
    regex::Regex::new(&pat)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

/// Check whether a finding's file has `// void-audit: ignore-file` at the top
/// or `// void-audit: ignore-next-line` on the line before the finding.
fn is_inline_suppressed(finding: &SecurityFinding, project_path: &Path) -> bool {
    let Some(ref rel_path) = finding.file_path else {
        return false;
    };
    let abs = project_path.join(rel_path);
    let content = match std::fs::read_to_string(&abs) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let lines: Vec<&str> = content.lines().collect();

    // File-level suppression: first 5 lines
    for line in lines.iter().take(5) {
        if line.contains("void-audit: ignore-file") {
            return true;
        }
    }

    // Line-level suppression
    if let Some(line_no) = finding.line_number
        && line_no >= 2
        && let Some(prev_line) = lines.get((line_no as usize) - 2)
        && prev_line.contains("void-audit: ignore-next-line")
    {
        return true;
    }

    false
}

/// Filter findings through `.void-audit-ignore` + inline directives.
/// Returns `(kept, suppressed_count)`.
pub fn filter_suppressed(
    findings: Vec<SecurityFinding>,
    project_path: &Path,
) -> (Vec<SecurityFinding>, usize) {
    let rules = load_rules(project_path);
    let mut kept = Vec::with_capacity(findings.len());
    let mut suppressed = 0usize;

    for finding in findings {
        // Check file-based rules
        let file_suppressed = rules.iter().any(|rule| {
            let id_match = glob_match(&rule.rule_glob, &finding.id);
            let path_match = finding
                .file_path
                .as_ref()
                .is_some_and(|fp| glob_match(&rule.path_glob, fp));
            id_match && path_match
        });

        if file_suppressed || is_inline_suppressed(&finding, project_path) {
            suppressed += 1;
            continue;
        }

        kept.push(finding);
    }

    (kept, suppressed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::findings::{FindingCategory, Severity};

    fn make_finding(id: &str, file: &str, line: u32) -> SecurityFinding {
        SecurityFinding::new(
            id.to_string(),
            Severity::Medium,
            FindingCategory::InsecureConfig,
            "test".to_string(),
            "test".to_string(),
            Some(file.to_string()),
            Some(line),
            "test".to_string(),
        )
    }

    #[test]
    fn test_glob_match_basics() {
        assert!(glob_match("*.rs", "foo.rs"));
        assert!(!glob_match("*.rs", "foo/bar.rs"));
        assert!(glob_match("**/*.rs", "foo/bar.rs"));
        assert!(glob_match(
            "crates/**/tests/**",
            "crates/core/tests/integration.rs"
        ));
        assert!(glob_match("secret-aws-*", "secret-aws-key-123"));
        assert!(!glob_match("secret-aws-*", "secret-gcp-key"));
    }

    #[test]
    fn test_file_based_suppression() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".void-audit-ignore"),
            "unwrap-*   src/audit/**\n",
        )
        .unwrap();
        // Create the file so inline check doesn't crash
        let src_dir = tmp.path().join("src").join("audit");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("xss.rs"), "fn x() {}\n").unwrap();

        let findings = vec![
            make_finding("unwrap-42", "src/audit/xss.rs", 10),
            make_finding("sql-inject-1", "src/main.rs", 5),
        ];
        let (kept, suppressed) = filter_suppressed(findings, tmp.path());
        assert_eq!(suppressed, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "sql-inject-1");
    }

    #[test]
    fn test_inline_ignore_next_line() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("lib.rs"),
            "fn foo() {\n    // void-audit: ignore-next-line\n    x.unwrap();\n}\n",
        )
        .unwrap();

        let findings = vec![make_finding("unwrap-1", "lib.rs", 3)];
        let (kept, suppressed) = filter_suppressed(findings, tmp.path());
        assert_eq!(suppressed, 1);
        assert!(kept.is_empty());
    }

    #[test]
    fn test_inline_ignore_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("generated.rs"),
            "// void-audit: ignore-file\n// auto-generated\nfn x() { y.unwrap(); }\n",
        )
        .unwrap();

        let findings = vec![
            make_finding("unwrap-1", "generated.rs", 3),
            make_finding("unwrap-2", "generated.rs", 5),
        ];
        let (kept, suppressed) = filter_suppressed(findings, tmp.path());
        assert_eq!(suppressed, 2);
        assert!(kept.is_empty());
    }

    #[test]
    fn test_no_ignore_file_means_no_suppression() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();

        let findings = vec![make_finding("vuln-1", "main.rs", 1)];
        let (kept, suppressed) = filter_suppressed(findings, tmp.path());
        assert_eq!(suppressed, 0);
        assert_eq!(kept.len(), 1);
    }
}
