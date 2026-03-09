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
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "__pycache__" {
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
                if trimmed.starts_with("DEBUG") && trimmed.contains("True") && !trimmed.starts_with('#') {
                    findings.push(SecurityFinding {
                        id: format!("debug-django-{}", findings.len()),
                        severity: Severity::High,
                        category: FindingCategory::DebugEnabled,
                        title: "Django DEBUG = True".into(),
                        description: format!("DEBUG está habilitado en {}", name),
                        file_path: Some(format!("{}/{}", dir_label, name)),
                        line_number: Some((i + 1) as u32),
                        remediation: "Usar DEBUG = os.environ.get('DEBUG', 'False') == 'True'".into(),
                    });
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
                    findings.push(SecurityFinding {
                        id: format!("debug-flask-{}", findings.len()),
                        severity: Severity::Medium,
                        category: FindingCategory::DebugEnabled,
                        title: "Flask debug=True".into(),
                        description: format!("Debug habilitado en {}", name),
                        file_path: Some(format!("{}/{}", dir_label, name)),
                        line_number: Some((i + 1) as u32),
                        remediation: "Usar variable de entorno para controlar debug mode".into(),
                    });
                }
            }
        }
    }

    // Node/Express: NODE_ENV not set to production check
    let pkg_path = dir.join("package.json");
    if let Some(content) = read_file_if_exists(&pkg_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(scripts) = json.get("scripts").and_then(|s| s.as_object()) {
                if let Some(start) = scripts.get("start").and_then(|s| s.as_str()) {
                    if !start.contains("NODE_ENV=production")
                        && !start.contains("cross-env NODE_ENV")
                        && start.contains("node ")
                    {
                        // Only warn if it's a server (not a build script)
                        if start.contains("server") || start.contains("app") || start.contains("index") {
                            findings.push(SecurityFinding {
                                id: format!("node-env-{}", findings.len()),
                                severity: Severity::Low,
                                category: FindingCategory::DebugEnabled,
                                title: "NODE_ENV no definido en start script".into(),
                                description: "El script 'start' no establece NODE_ENV=production".into(),
                                file_path: Some(format!("{}/package.json", dir_label)),
                                line_number: None,
                                remediation: "Agregar NODE_ENV=production al start script o usar cross-env".into(),
                            });
                        }
                    }
                }
            }
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
                    || (trimmed.contains("origins") && trimmed.contains("\"*\"") && trimmed.contains("cors"))
                {
                    findings.push(SecurityFinding {
                        id: format!("cors-open-{}", findings.len()),
                        severity: Severity::Medium,
                        category: FindingCategory::InsecureConfig,
                        title: "CORS permite todos los orígenes".into(),
                        description: format!("CORS wildcard (*) detectado en {}", name),
                        file_path: Some(format!("{}/{}", dir_label, name)),
                        line_number: Some((i + 1) as u32),
                        remediation: "Restringir CORS a dominios específicos en producción".into(),
                    });
                }
            }
        }
    }

    // JS/TS: cors({ origin: '*' }) or cors() without config
    let js_files = ["server.js", "server.ts", "app.js", "app.ts", "index.js", "index.ts"];
    for name in &js_files {
        let path = dir.join(name);
        if let Some(content) = read_file_if_exists(&path) {
            for (i, line) in content.lines().enumerate() {
                if (line.contains("cors()") && !line.contains("//"))
                    || (line.contains("cors(") && line.contains("'*'"))
                    || (line.contains("Access-Control-Allow-Origin") && line.contains("*"))
                {
                    findings.push(SecurityFinding {
                        id: format!("cors-js-{}", findings.len()),
                        severity: Severity::Medium,
                        category: FindingCategory::InsecureConfig,
                        title: "CORS permisivo".into(),
                        description: format!("CORS abierto detectado en {}", name),
                        file_path: Some(format!("{}/{}", dir_label, name)),
                        line_number: Some((i + 1) as u32),
                        remediation: "Configurar cors({ origin: ['https://tudominio.com'] })".into(),
                    });
                }
            }
        }
    }
}

fn scan_exposed_ports(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();

    // Check for 0.0.0.0 binding without awareness
    let files_to_check = [
        "main.py", "app.py", "server.py", "manage.py",
        "server.js", "server.ts", "app.js", "app.ts",
        "main.go", "main.rs",
    ];

    for name in &files_to_check {
        let path = dir.join(name);
        if let Some(content) = read_file_if_exists(&path) {
            for (i, line) in content.lines().enumerate() {
                if line.contains("0.0.0.0") && !line.trim().starts_with("//") && !line.trim().starts_with('#') {
                    // Binding to all interfaces — warning only
                    findings.push(SecurityFinding {
                        id: format!("bind-all-{}", findings.len()),
                        severity: Severity::Low,
                        category: FindingCategory::InsecureConfig,
                        title: "Binding a 0.0.0.0".into(),
                        description: format!("El servidor se enlaza a todas las interfaces en {}", name),
                        file_path: Some(format!("{}/{}", dir_label, name)),
                        line_number: Some((i + 1) as u32),
                        remediation: "En producción, usar 127.0.0.1 o configurar firewall".into(),
                    });
                }
            }
        }
    }
}

fn scan_missing_env_example(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();

    // .env exists but no .env.example
    if dir.join(".env").exists() && !dir.join(".env.example").exists() && !dir.join(".env.sample").exists() {
        findings.push(SecurityFinding {
            id: format!("env-no-example-{}", findings.len()),
            severity: Severity::Low,
            category: FindingCategory::InsecureConfig,
            title: "Falta .env.example".into(),
            description: "Existe .env pero no hay .env.example para documentar las variables necesarias".into(),
            file_path: Some(format!("{}/.env", dir_label)),
            line_number: None,
            remediation: "Crear .env.example con nombres de variables (sin valores reales)".into(),
        });
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
                findings.push(SecurityFinding {
                    id: format!("env-not-gitignored-{}", findings.len()),
                    severity: Severity::High,
                    category: FindingCategory::InsecureConfig,
                    title: ".env no está en .gitignore".into(),
                    description: "El archivo .env podría ser commiteado al repositorio con secretos".into(),
                    file_path: Some(format!("{}/.gitignore", dir_label)),
                    line_number: None,
                    remediation: "Agregar .env a .gitignore inmediatamente".into(),
                });
            }
        } else if !gitignore.exists() {
            findings.push(SecurityFinding {
                id: format!("no-gitignore-{}", findings.len()),
                severity: Severity::High,
                category: FindingCategory::InsecureConfig,
                title: "No existe .gitignore".into(),
                description: "Sin .gitignore, archivos sensibles (.env, keys) podrían ser commiteados".into(),
                file_path: Some(dir_label.clone()),
                line_number: None,
                remediation: "Crear .gitignore con .env, *.pem, *.key, etc.".into(),
            });
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
                findings.push(SecurityFinding {
                    id: format!("docker-root-{}", findings.len()),
                    severity: Severity::Medium,
                    category: FindingCategory::InsecureConfig,
                    title: "Container ejecuta como root".into(),
                    description: "El Dockerfile usa USER root".into(),
                    file_path: Some(format!("{}/Dockerfile", dir_label)),
                    line_number: Some((i + 1) as u32),
                    remediation: "Crear usuario non-root: RUN adduser --disabled-password appuser && USER appuser".into(),
                });
            }

            // Using latest tag
            if trimmed.starts_with("FROM ") && trimmed.contains(":LATEST") {
                findings.push(SecurityFinding {
                    id: format!("docker-latest-{}", findings.len()),
                    severity: Severity::Low,
                    category: FindingCategory::InsecureConfig,
                    title: "Imagen Docker usa tag :latest".into(),
                    description: "Usar :latest puede resultar en builds no reproducibles".into(),
                    file_path: Some(format!("{}/Dockerfile", dir_label)),
                    line_number: Some((i + 1) as u32),
                    remediation: "Fijar versión específica (ej: python:3.11-slim, node:20-alpine)".into(),
                });
            }

            // COPY . . without .dockerignore
            if trimmed.starts_with("COPY . .") || trimmed.starts_with("ADD . .") {
                if !dir.join(".dockerignore").exists() {
                    findings.push(SecurityFinding {
                        id: format!("docker-copy-all-{}", findings.len()),
                        severity: Severity::Medium,
                        category: FindingCategory::InsecureConfig,
                        title: "COPY . . sin .dockerignore".into(),
                        description: "Copiar todo el contexto puede incluir .env, .git, node_modules".into(),
                        file_path: Some(format!("{}/Dockerfile", dir_label)),
                        line_number: Some((i + 1) as u32),
                        remediation: "Crear .dockerignore con .env, .git, node_modules, target".into(),
                    });
                }
            }
        }

        // Check if Dockerfile has no USER instruction (runs as root by default)
        let has_user = content.lines().any(|l| l.trim().to_uppercase().starts_with("USER "));
        if !has_user {
            findings.push(SecurityFinding {
                id: format!("docker-no-user-{}", findings.len()),
                severity: Severity::Medium,
                category: FindingCategory::InsecureConfig,
                title: "Dockerfile sin instrucción USER".into(),
                description: "Sin USER, el container ejecuta como root por defecto".into(),
                file_path: Some(format!("{}/Dockerfile", dir_label)),
                line_number: None,
                remediation: "Agregar USER non-root al final del Dockerfile".into(),
            });
        }
    }
}

fn scan_package_json_scripts(dir: &Path, findings: &mut Vec<SecurityFinding>) {
    let dir_label = dir.to_string_lossy().to_string();
    let pkg = dir.join("package.json");

    if let Some(content) = read_file_if_exists(&pkg) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            // Check for pre/post install scripts (supply chain risk)
            if let Some(scripts) = json.get("scripts").and_then(|s| s.as_object()) {
                for key in ["preinstall", "postinstall"] {
                    if let Some(cmd) = scripts.get(key).and_then(|s| s.as_str()) {
                        if cmd.contains("curl") || cmd.contains("wget") || cmd.contains("http") {
                            findings.push(SecurityFinding {
                                id: format!("npm-{}-{}", key, findings.len()),
                                severity: Severity::High,
                                category: FindingCategory::InsecureConfig,
                                title: format!("Script {} sospechoso", key),
                                description: format!("El script {} descarga desde internet: {}", key, cmd),
                                file_path: Some(format!("{}/package.json", dir_label)),
                                line_number: None,
                                remediation: "Revisar el script y asegurar que las fuentes son confiables".into(),
                            });
                        }
                    }
                }
            }
        }
    }
}
