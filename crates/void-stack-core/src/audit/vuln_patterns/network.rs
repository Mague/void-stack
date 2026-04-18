//! SSRF (Server-Side Request Forgery) and open redirect pattern detectors.

use std::sync::OnceLock;

use regex::Regex;

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{FileInfo, adjust_severity, is_comment};

fn py_requests_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(requests|httpx)\.(get|post|put|delete|patch|head)\s*\(\s*[a-zA-Z_]"#)
            .expect("hardcoded regex")
    })
}

fn py_urllib_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"urllib\.request\.urlopen\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex")
    })
}

fn js_fetch_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\bfetch\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn js_axios_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"axios\.(get|post|put|delete|patch)\s*\(\s*[a-zA-Z_]"#)
            .expect("hardcoded regex")
    })
}

fn js_http_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"https?\.get\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn go_http_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"http\.(Get|Post|PostForm)\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex")
    })
}

fn go_client_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"client\.(Get|Post|Do)\s*\("#).expect("hardcoded regex"))
}

fn py_route_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"@(app|router)\.(get|post|put|delete|patch|route)\s*\("#)
            .expect("hardcoded regex")
    })
}

fn js_route_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(app|router)\.(get|post|put|delete|patch|use)\s*\("#)
            .expect("hardcoded regex")
    })
}

pub(crate) fn scan_ssrf(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_requests = py_requests_re();
    let py_urllib = py_urllib_re();
    let js_fetch = js_fetch_re();
    let js_axios = js_axios_re();
    let js_http = js_http_re();
    let go_http = go_http_re();
    let go_client = go_client_re();

    // Route decorators for context
    let py_route = py_route_re();
    let js_route = js_route_re();

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
                findings.push(SecurityFinding::new(
                    format!("ssrf-{}", findings.len()),
                    adjust_severity(Severity::High, file.is_test_file),
                    FindingCategory::Ssrf,
                    "Posible SSRF".into(),
                    format!(
                        "Request HTTP con URL variable en un handler de ruta en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Validar y allowlist URLs antes de hacer requests server-side. Nunca reenviar URLs suministradas por el usuario. Usar allowlist de hosts/schemes.".into(),
                ));
            }
        }
    }
}
