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
                    "Possible SSRF".into(),
                    format!(
                        "HTTP request with a variable URL inside a route handler in {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Validate and allowlist URLs before making server-side requests. Never forward user-supplied URLs. Use a host/scheme allowlist.".into(),
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
    fn test_ssrf_python_requests_in_route() {
        let file = make_file(
            "api.py",
            "py",
            r#"@app.get("/proxy")
def proxy():
    url = flask_request.args["target"]
    resp = requests.get(url)
    return resp.text"#,
        );
        let mut findings = Vec::new();
        scan_ssrf(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].category, FindingCategory::Ssrf));
        assert!(matches!(findings[0].severity, Severity::High));
        assert_eq!(findings[0].line_number, Some(4));
    }

    #[test]
    fn test_ssrf_python_without_route_ok() {
        // The same HTTP call outside a route handler file is low confidence
        // and must not be flagged.
        let file = make_file("client.py", "py", "resp = requests.get(url)");
        let mut findings = Vec::new();
        scan_ssrf(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_ssrf_js_fetch_in_route() {
        let file = make_file(
            "server.ts",
            "ts",
            r#"app.get('/proxy', async (req, res) => {
    const r = await fetch(target)
    res.send(await r.text())
})"#,
        );
        let mut findings = Vec::new();
        scan_ssrf(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line_number, Some(2));
    }

    #[test]
    fn test_ssrf_js_axios_in_route() {
        let file = make_file(
            "server.js",
            "js",
            r#"router.post('/relay', handler)
const r = await axios.get(target)"#,
        );
        let mut findings = Vec::new();
        scan_ssrf(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_ssrf_hardcoded_url_skipped() {
        // Lines containing a hardcoded http(s) URL string are skipped even
        // inside route handler files.
        let file = make_file(
            "api.py",
            "py",
            r#"@app.get("/status")
def status():
    resp = requests.get(url, headers={"referer": "http://internal"})
    return resp.text"#,
        );
        let mut findings = Vec::new();
        scan_ssrf(&[file], &mut findings);
        assert!(findings.is_empty(), "hardcoded URL lines must be skipped");
    }

    #[test]
    fn test_ssrf_skips_comments() {
        let file = make_file(
            "api.py",
            "py",
            r#"@app.get("/proxy")
def proxy():
    # resp = requests.get(url)
    pass"#,
        );
        let mut findings = Vec::new();
        scan_ssrf(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_ssrf_go_not_flagged_without_route_detection() {
        // Route detection only exists for Python and JS/TS, so Go HTTP calls
        // never reach the confidence threshold. Documents current behavior.
        let file = make_file("main.go", "go", "resp, err := http.Get(target)");
        let mut findings = Vec::new();
        scan_ssrf(&[file], &mut findings);
        assert!(findings.is_empty());
    }
}
