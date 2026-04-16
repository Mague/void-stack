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
                findings.push(SecurityFinding {
                    id: format!("xss-{}", findings.len()),
                    severity: adjust_severity(base_severity, file.is_test_file),
                    category: FindingCategory::XssVulnerability,
                    title: "Posible XSS".into(),
                    description: format!(
                        "Asignaci\u{00f3}n de HTML no sanitizado o eval() en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Nunca asignar input del usuario a innerHTML. Usar textContent. Sanitizar HTML con DOMPurify si se necesita rich content. Evitar eval() y new Function().".into(),
                });
            } else if dangerously.is_match(line) {
                findings.push(SecurityFinding {
                    id: format!("xss-{}", findings.len()),
                    severity: adjust_severity(Severity::Low, file.is_test_file),
                    category: FindingCategory::XssVulnerability,
                    title: "dangerouslySetInnerHTML".into(),
                    description: format!(
                        "Uso de dangerouslySetInnerHTML en {}:{} \u{2014} React escapa por defecto, pero revisar",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Asegurar que el contenido est\u{00e1} sanitizado con DOMPurify antes de usar dangerouslySetInnerHTML.".into(),
                });
            }
        }
    }
}
