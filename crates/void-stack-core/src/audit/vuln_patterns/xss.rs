//! XSS (Cross-Site Scripting) pattern detectors.

use regex::Regex;

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{adjust_severity, is_comment, FileInfo};

pub(crate) fn scan_xss(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let inner_html = Regex::new(r#"\.innerHTML\s*[+=]"#).unwrap();
    let outer_html = Regex::new(r#"\.outerHTML\s*="#).unwrap();
    let doc_write = Regex::new(r#"document\.write\s*\(\s*[a-zA-Z_]"#).unwrap();
    let insert_html = Regex::new(r#"insertAdjacentHTML\s*\(\s*['"][^'"]+['"]\s*,\s*[a-zA-Z_]"#).unwrap();
    let dangerously = Regex::new(r#"dangerouslySetInnerHTML\s*=\s*\{\s*\{?\s*__html\s*:"#).unwrap();
    let eval_var = Regex::new(r#"\beval\s*\(\s*[a-zA-Z_]"#).unwrap();
    let new_function = Regex::new(r#"new\s+Function\s*\(\s*[a-zA-Z_]"#).unwrap();

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
            let is_controlled = uses_controlled_render
                && (inner_html.is_match(line) || outer_html.is_match(line));

            if inner_html.is_match(line) && !has_literal_only
                || outer_html.is_match(line)
                || doc_write.is_match(line)
                || insert_html.is_match(line)
                || eval_var.is_match(line)
                || new_function.is_match(line)
            {
                let base_severity = if is_controlled { Severity::Low } else { Severity::High };
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
