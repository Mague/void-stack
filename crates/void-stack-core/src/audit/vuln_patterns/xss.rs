//! XSS (Cross-Site Scripting) pattern detectors.

use std::sync::OnceLock;

use regex::Regex;

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{FileInfo, adjust_severity, is_comment};

fn inner_html_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\.innerHTML\s*[+=]"#).expect("hardcoded regex"))
}

fn outer_html_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\.outerHTML\s*="#).expect("hardcoded regex"))
}

fn doc_write_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"document\.write\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn insert_adjacent_html_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"insertAdjacentHTML\s*\(\s*['"][^'"]+['"]\s*,\s*[a-zA-Z_]"#)
            .expect("hardcoded regex")
    })
}

fn dangerously_set_inner_html_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"dangerouslySetInnerHTML\s*=\s*\{\s*\{?\s*__html\s*:"#)
            .expect("hardcoded regex")
    })
}

fn eval_var_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\beval\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn new_function_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"new\s+Function\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

pub(crate) fn scan_xss(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let inner_html = inner_html_re();
    let outer_html = outer_html_re();
    let doc_write = doc_write_re();
    let insert_html = insert_adjacent_html_re();
    let dangerously = dangerously_set_inner_html_re();
    let eval_var = eval_var_re();
    let new_function = new_function_re();

    for file in files {
        if !matches!(file.ext.as_str(), "js" | "ts" | "jsx" | "tsx") {
            continue;
        }

        // Skip files that use controlled rendering libraries (mermaid, chart.js)
        let uses_controlled_render = file.content.contains("mermaid")
            || file.content.contains("chart.js")
            || file.content.contains("Chart.js")
            || file.content.contains("d3.select");

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            // Check for string literal (not a variable)
            let has_literal_only = line.contains("innerHTML = \"")
                || line.contains("innerHTML = '")
                || line.contains("innerHTML = `");

            // For controlled render contexts, reduce severity instead of skipping
            let is_controlled =
                uses_controlled_render && (inner_html.is_match(line) || outer_html.is_match(line));

            if inner_html.is_match(line) && !has_literal_only
                || outer_html.is_match(line)
                || doc_write.is_match(line)
                || insert_html.is_match(line)
                || eval_var.is_match(line)
                || new_function.is_match(line)
            {
                let base_severity = if is_controlled {
                    Severity::Low
                } else {
                    Severity::High
                };
                findings.push(SecurityFinding::new(
                    format!("xss-{}", findings.len()),
                    adjust_severity(base_severity, file.is_test_file),
                    FindingCategory::XssVulnerability,
                    "Possible XSS".into(),
                    format!(
                        "Assignment of unsanitized HTML or eval() in {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Never assign user input to innerHTML. Use textContent. Sanitize HTML with DOMPurify if rich content is needed. Avoid eval() and new Function().".into(),
                ));
            } else if dangerously.is_match(line) {
                findings.push(SecurityFinding::new(
                    format!("xss-{}", findings.len()),
                    adjust_severity(Severity::Low, file.is_test_file),
                    FindingCategory::XssVulnerability,
                    "dangerouslySetInnerHTML".into(),
                    format!(
                        "Use of dangerouslySetInnerHTML in {}:{} \u{2014} React escapes by default, but review",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Make sure the content is sanitized with DOMPurify before using dangerouslySetInnerHTML.".into(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file(path: &str, ext: &str, content: &str) -> FileInfo {
        FileInfo {
            rel_path: path.into(),
            content: content.into(),
            ext: ext.into(),
            is_test_file: false,
        }
    }

    #[test]
    fn test_xss_innerhtml_variable() {
        let file = make_file("view.js", "js", "element.innerHTML = userContent");
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0].category,
            FindingCategory::XssVulnerability
        ));
        assert!(matches!(findings[0].severity, Severity::High));
        assert_eq!(findings[0].line_number, Some(1));
    }

    #[test]
    fn test_xss_innerhtml_string_literal_ok() {
        // A hardcoded string literal cannot carry user input.
        let file = make_file("view.js", "js", r#"element.innerHTML = "<b>static</b>""#);
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert!(findings.is_empty(), "literal innerHTML must not be flagged");
    }

    #[test]
    fn test_xss_innerhtml_append_variable() {
        // += with a variable is still an injection sink.
        let file = make_file("view.js", "js", "element.innerHTML += chunk");
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_xss_outerhtml() {
        let file = make_file("view.js", "js", "node.outerHTML = markup");
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_xss_document_write() {
        let file = make_file("legacy.js", "js", "document.write(payload)");
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_xss_insert_adjacent_html() {
        let file = make_file(
            "view.ts",
            "ts",
            "container.insertAdjacentHTML('beforeend', html)",
        );
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_xss_eval_variable() {
        let file = make_file("runtime.js", "js", "eval(code)");
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_xss_new_function() {
        let file = make_file("runtime.js", "js", "const fn = new Function(body)");
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_xss_dangerously_set_inner_html_is_low() {
        let file = make_file(
            "Component.tsx",
            "tsx",
            "<div dangerouslySetInnerHTML={{ __html: content }} />",
        );
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "dangerouslySetInnerHTML");
        // React escapes by default, so this is only informational.
        assert!(matches!(findings[0].severity, Severity::Low));
    }

    #[test]
    fn test_xss_controlled_render_reduces_severity() {
        // Files using controlled render libraries (mermaid) get Low severity
        // for innerHTML because the library output is trusted.
        let file = make_file(
            "diagram.js",
            "js",
            "import mermaid from 'mermaid'\ncontainer.innerHTML = svg",
        );
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, Severity::Low));
    }

    #[test]
    fn test_xss_skips_comments() {
        let file = make_file("view.js", "js", "// element.innerHTML = userContent");
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_xss_ignores_non_js_files() {
        // The XSS scanner only inspects JS/TS/JSX/TSX files.
        let file = make_file("template.py", "py", "element.innerHTML = user_content");
        let mut findings = Vec::new();
        scan_xss(&[file], &mut findings);
        assert!(findings.is_empty());
    }
}
