//! SSRF (Server-Side Request Forgery) and open redirect pattern detectors.

use regex::Regex;

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{adjust_severity, is_comment, FileInfo};

pub(crate) fn scan_ssrf(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_requests = Regex::new(r#"(?i)(requests|httpx)\.(get|post|put|delete|patch|head)\s*\(\s*[a-zA-Z_]"#).unwrap();
    let py_urllib = Regex::new(r#"urllib\.request\.urlopen\s*\(\s*[a-zA-Z_]"#).unwrap();
    let js_fetch = Regex::new(r#"\bfetch\s*\(\s*[a-zA-Z_]"#).unwrap();
    let js_axios = Regex::new(r#"axios\.(get|post|put|delete|patch)\s*\(\s*[a-zA-Z_]"#).unwrap();
    let js_http = Regex::new(r#"https?\.get\s*\(\s*[a-zA-Z_]"#).unwrap();
    let go_http = Regex::new(r#"http\.(Get|Post|PostForm)\s*\(\s*[a-zA-Z_]"#).unwrap();
    let go_client = Regex::new(r#"client\.(Get|Post|Do)\s*\("#).unwrap();

    // Route decorators for context
    let py_route = Regex::new(r#"@(app|router)\.(get|post|put|delete|patch|route)\s*\("#).unwrap();
    let js_route = Regex::new(r#"(app|router)\.(get|post|put|delete|patch|use)\s*\("#).unwrap();

    for file in files {
        // Check if file has route handlers (higher confidence for SSRF)
        let has_routes = match file.ext.as_str() {
            "py" => py_route.is_match(&file.content),
            "js" | "ts" => js_route.is_match(&file.content),
            _ => false,
        };

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            // Skip lines that use a hardcoded URL string
            if line.contains("\"http") || line.contains("'http") || line.contains("`http") {
                continue;
            }

            let matched = match file.ext.as_str() {
                "py" => py_requests.is_match(line) || py_urllib.is_match(line),
                "js" | "ts" | "jsx" | "tsx" => {
                    js_fetch.is_match(line) || js_axios.is_match(line) || js_http.is_match(line)
                }
                "go" => go_http.is_match(line) || go_client.is_match(line),
                _ => false,
            };

            if matched && has_routes {
                findings.push(SecurityFinding {
                    id: format!("ssrf-{}", findings.len()),
                    severity: adjust_severity(Severity::High, file.is_test_file),
                    category: FindingCategory::Ssrf,
                    title: "Posible SSRF".into(),
                    description: format!(
                        "Request HTTP con URL variable en un handler de ruta en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Validar y allowlist URLs antes de hacer requests server-side. Nunca reenviar URLs suministradas por el usuario. Usar allowlist de hosts/schemes.".into(),
                });
            }
        }
    }
}
