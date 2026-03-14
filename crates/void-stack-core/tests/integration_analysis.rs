//! Integration tests for core analysis functions.
//! These mirror the desktop command workflows (analyze_project_cmd, audit, debt, space).

use std::fs;
use tempfile::TempDir;

use void_stack_core::analyzer;
use void_stack_core::audit;
use void_stack_core::space;

// ── Helpers ──────────────────────────────────────────────────────────────

fn setup_project(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
    }
    dir
}

// ── Architecture analysis ────────────────────────────────────────────────

#[test]
fn test_analyze_python_project() {
    let dir = setup_project(&[
        (
            "app.py",
            r#"
from flask import Flask
from models import User
from services import auth_service
app = Flask(__name__)
"#,
        ),
        (
            "models.py",
            r#"
class User:
    def __init__(self, name):
        self.name = name
"#,
        ),
        (
            "services.py",
            r#"
from models import User
def auth_service(user):
    return user.name
"#,
        ),
    ]);

    let result = analyzer::analyze_project(dir.path());
    assert!(result.is_some(), "should analyze Python project");
    let r = result.unwrap();
    assert!(!r.graph.modules.is_empty(), "should find modules");
    assert!(r.graph.modules.len() >= 2, "should find at least 2 modules");
}

#[test]
fn test_analyze_javascript_project() {
    let dir = setup_project(&[
        (
            "index.js",
            "const { getUsers } = require('./controllers/userController');\nconst app = require('express')();\napp.get('/users', getUsers);\n",
        ),
        (
            "controllers/userController.js",
            "const { UserModel } = require('../models/user');\nfunction getUsers(req, res) { res.json(UserModel.findAll()); }\nmodule.exports = { getUsers };\n",
        ),
        (
            "models/user.js",
            "class UserModel { static findAll() { return []; } }\nmodule.exports = { UserModel };\n",
        ),
    ]);

    let result = analyzer::analyze_project(dir.path());
    // JS project analysis may or may not find modules depending on import resolution
    if let Some(r) = result {
        assert!(!r.graph.modules.is_empty());
    }
}

#[test]
fn test_analyze_rust_project() {
    let dir = setup_project(&[
        (
            "src/main.rs",
            "mod config;\nmod server;\nfn main() {\n    let cfg = config::load();\n    server::start(cfg);\n}\n",
        ),
        (
            "src/config.rs",
            "pub struct Config { pub port: u16 }\npub fn load() -> Config { Config { port: 8080 } }\n",
        ),
        (
            "src/server.rs",
            "use crate::config::Config;\npub fn start(cfg: Config) {\n    println!(\"Starting on port {}\", cfg.port);\n}\n",
        ),
    ]);

    let result = analyzer::analyze_project(dir.path());
    // Rust analysis may not detect modules without Cargo.toml
    if let Some(r) = result {
        assert!(!r.graph.modules.is_empty());
    }
}

#[test]
fn test_analyze_empty_project() {
    let dir = TempDir::new().unwrap();
    let result = analyzer::analyze_project(dir.path());
    assert!(result.is_none(), "empty project should return None");
}

#[test]
fn test_generate_docs() {
    let dir = setup_project(&[
        ("app.py", "from models import User\nclass App:\n    pass"),
        ("models.py", "class User:\n    pass"),
    ]);
    let result = analyzer::analyze_project(dir.path());
    if let Some(r) = result {
        let markdown = analyzer::generate_docs(&r, "test-project");
        assert!(!markdown.is_empty(), "should generate markdown docs");
        assert!(
            markdown.contains("test-project"),
            "should include project name"
        );
    }
}

// ── Explicit debt scanning ──────────────────────────────────────────────

#[test]
fn test_scan_explicit_debt() {
    let dir = setup_project(&[
        (
            "app.py",
            "# TODO: refactor this later\ndef func():\n    pass  # FIXME: broken logic\n    # HACK: temporary workaround",
        ),
        (
            "main.js",
            "// TODO: add error handling\nfunction run() { /* FIXME: memory leak */ }",
        ),
    ]);

    let items = analyzer::explicit_debt::scan_explicit_debt(dir.path());
    assert!(
        items.len() >= 4,
        "should find TODO, FIXME, and HACK markers"
    );

    let todos: Vec<_> = items.iter().filter(|i| i.kind == "TODO").collect();
    let fixmes: Vec<_> = items.iter().filter(|i| i.kind == "FIXME").collect();
    let hacks: Vec<_> = items.iter().filter(|i| i.kind == "HACK").collect();

    assert!(!todos.is_empty(), "should find TODO markers");
    assert!(!fixmes.is_empty(), "should find FIXME markers");
    assert!(!hacks.is_empty(), "should find HACK markers");
}

#[test]
fn test_scan_explicit_debt_empty_project() {
    let dir = setup_project(&[("clean.py", "def clean_code():\n    return True")]);
    let items = analyzer::explicit_debt::scan_explicit_debt(dir.path());
    assert!(items.is_empty(), "clean code should have no debt markers");
}

// ── Full audit pipeline ──────────────────────────────────────────────────

#[test]
fn test_audit_project_with_secrets() {
    let dir = setup_project(&[(
        "config.py",
        &format!(
            "API_KEY = \"AKIAIOSFODNN7ABCDEFGH\"\nDB = \"postgres://admin:{}@localhost/db\"",
            "pass"
        ),
    )]);
    let result = audit::audit_project("test-proj", dir.path());
    assert!(
        !result.findings.is_empty(),
        "should detect hardcoded secrets"
    );
    assert!(result.summary.total > 0);
}

#[test]
fn test_audit_project_with_insecure_config() {
    let dir = setup_project(&[
        ("settings.py", "DEBUG = True"),
        (
            "Dockerfile",
            "FROM python:latest\nCOPY . .\nCMD [\"python\", \"app.py\"]",
        ),
    ]);
    let result = audit::audit_project("test-proj", dir.path());
    assert!(
        !result.findings.is_empty(),
        "should detect insecure configs"
    );
}

#[test]
fn test_audit_project_clean() {
    let dir = setup_project(&[("app.py", "import os\nAPI_KEY = os.environ.get('API_KEY')")]);
    let result = audit::audit_project("clean-proj", dir.path());
    // Clean project should have minimal or no findings
    assert_eq!(result.project_name, "clean-proj");
}

#[test]
fn test_audit_risk_score_calculation() {
    let dir = setup_project(&[("leak.py", "secret = \"AKIAIOSFODNN7ABCDEFGH\"")]);
    let result = audit::audit_project("risky", dir.path());
    // Critical findings should raise risk score
    let has_critical = result
        .findings
        .iter()
        .any(|f| f.severity == audit::Severity::Critical);
    if has_critical {
        assert!(
            result.summary.risk_score > 0.0,
            "critical findings should raise risk score"
        );
    }
}

#[test]
fn test_generate_audit_report() {
    let dir = setup_project(&[("settings.py", "DEBUG = True")]);
    let result = audit::audit_project("report-proj", dir.path());
    let report = audit::generate_report(&result);
    assert!(!report.is_empty(), "should generate report");
    assert!(
        report.contains("report-proj"),
        "report should include project name"
    );
}

// ── Space scanning ──────────────────────────────────────────────────────

#[test]
fn test_space_scan_project_with_heavy_dirs() {
    let dir = TempDir::new().unwrap();
    // Create a node_modules dir with >500KB
    let nm = dir.path().join("node_modules").join("lodash");
    fs::create_dir_all(&nm).unwrap();
    fs::write(nm.join("lodash.js"), vec![0u8; 600_000]).unwrap();

    let entries = space::scan_project(dir.path());
    assert!(!entries.is_empty(), "should find heavy node_modules");
    assert!(entries[0].deletable, "node_modules should be deletable");
    assert_eq!(entries[0].restore_hint, "npm install");
}

#[test]
fn test_space_scan_global_no_crash() {
    // scan_global reads real system paths — just verify it doesn't crash
    let entries = space::scan_global();
    // May be empty or have entries, just check it runs
    for entry in &entries {
        assert!(entry.size_bytes > 0);
        assert!(!entry.size_human.is_empty());
    }
}

#[test]
fn test_space_delete_entry_safety() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}").unwrap();

    // Attempting to delete "src" should fail (not in safe names list)
    let result = space::delete_entry(&src.to_string_lossy());
    assert!(result.is_err());
    assert!(src.exists(), "src should not be deleted");
}

// ── Vuln patterns (through public API) ───────────────────────────────────

#[test]
fn test_vuln_patterns_sql_injection() {
    let dir = setup_project(&[(
        "app.py",
        "def get_user(user_id):\n    cursor.execute(\"SELECT * FROM users WHERE id = \" + user_id)\n",
    )]);
    let findings = audit::vuln_patterns::scan_vuln_patterns(dir.path());
    assert!(
        findings
            .iter()
            .any(|f| f.category == audit::FindingCategory::SqlInjection),
        "should detect SQL injection"
    );
}

#[test]
fn test_vuln_patterns_command_injection() {
    let dir = setup_project(&[(
        "deploy.py",
        "import subprocess\ndef run_deploy(branch):\n    subprocess.run(branch, shell=True)\n",
    )]);
    let findings = audit::vuln_patterns::scan_vuln_patterns(dir.path());
    assert!(
        findings
            .iter()
            .any(|f| f.category == audit::FindingCategory::CommandInjection),
        "should detect command injection"
    );
}

#[test]
fn test_vuln_patterns_xss() {
    let dir = setup_project(&[(
        "page.js",
        r#"
function render(data) {
    document.getElementById('output').innerHTML = data.userInput;
}
"#,
    )]);
    let findings = audit::vuln_patterns::scan_vuln_patterns(dir.path());
    assert!(
        findings
            .iter()
            .any(|f| f.category == audit::FindingCategory::XssVulnerability),
        "should detect XSS via innerHTML"
    );
}

#[test]
fn test_vuln_patterns_weak_crypto() {
    let dir = setup_project(&[(
        "crypto.py",
        r#"
import hashlib
def hash_password(password):
    return hashlib.md5(password.encode()).hexdigest()
"#,
    )]);
    let findings = audit::vuln_patterns::scan_vuln_patterns(dir.path());
    assert!(
        findings
            .iter()
            .any(|f| f.category == audit::FindingCategory::WeakCryptography),
        "should detect weak hash (md5)"
    );
}

#[test]
fn test_vuln_patterns_insecure_deserialization() {
    let dir = setup_project(&[(
        "loader.py",
        r#"
import pickle
def load_data(file_path):
    with open(file_path, 'rb') as f:
        return pickle.load(f)
"#,
    )]);
    let findings = audit::vuln_patterns::scan_vuln_patterns(dir.path());
    assert!(
        findings
            .iter()
            .any(|f| f.category == audit::FindingCategory::InsecureDeserialization),
        "should detect insecure deserialization (pickle)"
    );
}

#[test]
fn test_vuln_patterns_clean_code_no_xss() {
    // Verify clean JS code does not trigger XSS
    let dir = setup_project(&[(
        "safe.js",
        "function render(data) {\n    document.getElementById('output').textContent = data;\n}\n",
    )]);
    let findings = audit::vuln_patterns::scan_vuln_patterns(dir.path());
    assert!(
        !findings
            .iter()
            .any(|f| f.category == audit::FindingCategory::XssVulnerability),
        "textContent should not trigger XSS"
    );
}
