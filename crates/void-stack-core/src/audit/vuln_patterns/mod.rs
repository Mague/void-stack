//! Static analysis detectors for code vulnerability patterns:
//! SQL injection, command injection, path traversal, insecure deserialization,
//! weak cryptography, XSS, SSRF, exposed debug endpoints, secrets in git history.

mod config;
mod crypto;
mod injection;
mod network;
mod xss;

use std::path::Path;

use super::findings::{SecurityFinding, Severity};

// ── Shared infrastructure ───────────────────────────────────

const CODE_EXTENSIONS: &[&str] = &[
    "py", "js", "ts", "jsx", "tsx", "go", "rs", "java", "rb", "php",
];

const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", "dist", "build", "__pycache__",
    ".venv", "venv", ".next", ".nuxt", "vendor", ".dart_tool",
    ".gradle", ".idea", ".vscode", "coverage", ".tox",
];

pub(crate) struct FileInfo {
    pub rel_path: String,
    pub content: String,
    pub ext: String,
    pub is_test_file: bool,
}

/// Collect all scannable source files from the project.
fn collect_source_files(root: &Path) -> Vec<FileInfo> {
    let mut files = Vec::new();
    collect_files_recursive(root, root, &mut files, 0);
    files
}

fn collect_files_recursive(root: &Path, dir: &Path, files: &mut Vec<FileInfo>, depth: u32) {
    if depth > 6 {
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
            collect_files_recursive(root, &path, files, depth + 1);
            continue;
        }

        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if !CODE_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        // Skip .min.js
        let filename_lower = name_str.to_lowercase();
        if filename_lower.ends_with(".min.js") {
            continue;
        }

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

        let rel_lower = rel_path.to_lowercase();
        let is_test_file = rel_lower.contains("test/")
            || rel_lower.contains("tests/")
            || rel_lower.contains("__tests__/")
            || rel_lower.contains(".test.")
            || rel_lower.contains(".spec.")
            || rel_lower.contains("mock")
            || rel_lower.contains("fixture");

        files.push(FileInfo {
            rel_path,
            content,
            ext,
            is_test_file,
        });
    }
}

pub(crate) fn adjust_severity(base: Severity, is_test: bool) -> Severity {
    if !is_test {
        return base;
    }
    match base {
        Severity::Critical => Severity::High,
        Severity::High => Severity::Medium,
        Severity::Medium => Severity::Low,
        s => s,
    }
}

pub(crate) fn is_comment(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("//") || t.starts_with('#') || t.starts_with("/*") || t.starts_with('*')
}

/// Run all vulnerability pattern scanners.
pub fn scan_vuln_patterns(project_path: &Path) -> Vec<SecurityFinding> {
    let files = collect_source_files(project_path);
    let mut findings = Vec::new();

    injection::scan_sql_injection(&files, &mut findings);
    injection::scan_command_injection(&files, &mut findings);
    injection::scan_path_traversal(&files, &mut findings);
    crypto::scan_insecure_deserialization(&files, &mut findings);
    crypto::scan_weak_cryptography(&files, &mut findings);
    xss::scan_xss(&files, &mut findings);
    network::scan_ssrf(&files, &mut findings);
    config::scan_debug_endpoints(&files, &mut findings);
    config::scan_git_history(project_path, &mut findings);

    findings
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::findings::FindingCategory;
    use std::fs;
    use tempfile::tempdir;

    fn scan_file(filename: &str, content: &str) -> Vec<SecurityFinding> {
        let dir = tempdir().unwrap();
        let file = dir.path().join(filename);
        fs::write(&file, content).unwrap();
        scan_vuln_patterns(dir.path())
    }

    #[test]
    fn test_sql_injection_python_fstring() {
        let findings = scan_file("app.py", r#"
def get_user(user_id):
    db.execute(f"SELECT * FROM users WHERE id = {user_id}")
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::SqlInjection)));
    }

    #[test]
    fn test_sql_injection_js_template() {
        let findings = scan_file("api.ts", r#"
const result = await db.query(`SELECT * FROM users WHERE name = ${name}`)
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::SqlInjection)));
    }

    #[test]
    fn test_command_injection_subprocess() {
        let findings = scan_file("utils.py", r#"
import subprocess
subprocess.run(cmd, shell=True)
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::CommandInjection)));
    }

    #[test]
    fn test_command_injection_eval_js() {
        let findings = scan_file("handler.js", r#"
eval(userInput)
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::CommandInjection)));
    }

    #[test]
    fn test_path_traversal_python() {
        let findings = scan_file("views.py", r#"
@app.get("/file")
def get_file():
    return send_file(request.args.get('path'))
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::PathTraversal)));
    }

    #[test]
    fn test_insecure_deserialization_pickle() {
        let findings = scan_file("loader.py", r#"
import pickle
data = pickle.loads(raw_bytes)
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::InsecureDeserialization)));
    }

    #[test]
    fn test_insecure_deserialization_yaml() {
        let findings = scan_file("config.py", r#"
import yaml
data = yaml.load(content)
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::InsecureDeserialization)));
    }

    #[test]
    fn test_weak_crypto_md5_password() {
        let findings = scan_file("auth.py", r#"
import hashlib
hashed = hashlib.md5(password.encode()).hexdigest()
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::WeakCryptography)));
    }

    #[test]
    fn test_weak_crypto_math_random_token() {
        let findings = scan_file("token.js", r#"
const token = Math.random().toString(36)
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::WeakCryptography)));
    }

    #[test]
    fn test_xss_innerhtml() {
        let findings = scan_file("component.js", r#"
element.innerHTML = userContent
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::XssVulnerability)));
    }

    #[test]
    fn test_xss_dangerously() {
        let findings = scan_file("Component.tsx", r#"
<div dangerouslySetInnerHTML={{ __html: content }} />
"#);
        let xss = findings.iter().find(|f| matches!(f.category, FindingCategory::XssVulnerability));
        assert!(xss.is_some());
        assert!(matches!(xss.unwrap().severity, Severity::Low));
    }

    #[test]
    fn test_ssrf_in_route() {
        let findings = scan_file("api.py", r#"
from flask import Flask, request
import requests

app = Flask(__name__)

@app.get("/proxy")
def proxy():
    url = request.args.get('url')
    resp = requests.get(url)
    return resp.text
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::Ssrf)));
    }

    #[test]
    fn test_debug_endpoint_python() {
        let findings = scan_file("routes.py", r#"
@app.get("/debug")
def debug_view():
    return {"env": os.environ}
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::ExposedDebugEndpoint)));
    }

    #[test]
    fn test_debug_endpoint_js() {
        let findings = scan_file("server.ts", r#"
app.get('/actuator/env', (req, res) => {
    res.json(process.env)
})
"#);
        assert!(findings.iter().any(|f| matches!(f.category, FindingCategory::ExposedDebugEndpoint)));
    }

    #[test]
    fn test_test_file_severity_reduction() {
        let dir = tempdir().unwrap();
        let test_dir = dir.path().join("tests");
        fs::create_dir(&test_dir).unwrap();
        fs::write(test_dir.join("test_app.py"), r#"
import subprocess
subprocess.run(cmd, shell=True)
"#).unwrap();
        let findings = scan_vuln_patterns(dir.path());
        let cmdi = findings.iter().find(|f| matches!(f.category, FindingCategory::CommandInjection));
        assert!(cmdi.is_some());
        // Should be reduced from Critical to High
        assert!(matches!(cmdi.unwrap().severity, Severity::High));
    }

    #[test]
    fn test_skip_min_js() {
        let findings = scan_file("vendor.min.js", r#"
eval(code)
element.innerHTML = data
"#);
        assert!(findings.is_empty());
    }
}
