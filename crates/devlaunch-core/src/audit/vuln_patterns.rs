//! Static analysis detectors for code vulnerability patterns:
//! SQL injection, command injection, path traversal, insecure deserialization,
//! weak cryptography, XSS, SSRF, exposed debug endpoints, secrets in git history.

use std::path::Path;
use std::process::Command;

use regex::Regex;

use super::findings::{FindingCategory, SecurityFinding, Severity};

// ── Shared infrastructure ───────────────────────────────────

const CODE_EXTENSIONS: &[&str] = &[
    "py", "js", "ts", "jsx", "tsx", "go", "rs", "java", "rb", "php",
];

const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", "dist", "build", "__pycache__",
    ".venv", "venv", ".next", ".nuxt", "vendor", ".dart_tool",
    ".gradle", ".idea", ".vscode", "coverage", ".tox",
];

struct FileInfo {
    rel_path: String,
    content: String,
    ext: String,
    is_test_file: bool,
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

fn adjust_severity(base: Severity, is_test: bool) -> Severity {
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

fn is_comment(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("//") || t.starts_with('#') || t.starts_with("/*") || t.starts_with('*')
}

/// Run all vulnerability pattern scanners.
pub fn scan_vuln_patterns(project_path: &Path) -> Vec<SecurityFinding> {
    let files = collect_source_files(project_path);
    let mut findings = Vec::new();

    scan_sql_injection(&files, &mut findings);
    scan_command_injection(&files, &mut findings);
    scan_path_traversal(&files, &mut findings);
    scan_insecure_deserialization(&files, &mut findings);
    scan_weak_cryptography(&files, &mut findings);
    scan_xss(&files, &mut findings);
    scan_ssrf(&files, &mut findings);
    scan_debug_endpoints(&files, &mut findings);
    scan_git_history(project_path, &mut findings);

    findings
}

// ── 1. SQL Injection ────────────────────────────────────────

fn scan_sql_injection(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_fstring = Regex::new(r#"(?i)f["'][^"']*\b(SELECT|INSERT|UPDATE|DELETE|WHERE)\b[^"']*\{"#).unwrap();
    let py_execute_concat = Regex::new(r#"(?i)\.execute\s*\([^)]*(\+|\.format\s*\(|%\s*[(\w])"#).unwrap();
    let py_raw = Regex::new(r#"(?i)\.raw\s*\(\s*[a-zA-Z_]"#).unwrap();
    let js_template_sql = Regex::new(r#"(?i)`[^`]*\b(SELECT|INSERT|UPDATE|DELETE|WHERE)\b[^`]*\$\{"#).unwrap();
    let js_query_concat = Regex::new(r#"(?i)\.(query|execute)\s*\([^)]*\+"#).unwrap();

    for file in files {
        if !matches!(file.ext.as_str(), "py" | "js" | "ts" | "jsx" | "tsx") {
            continue;
        }
        let is_python = file.ext == "py";

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            let matched = if is_python {
                py_fstring.is_match(line)
                    || py_execute_concat.is_match(line)
                    || py_raw.is_match(line)
            } else {
                js_template_sql.is_match(line) || js_query_concat.is_match(line)
            };

            if matched {
                findings.push(SecurityFinding {
                    id: format!("sqli-{}", findings.len()),
                    severity: adjust_severity(Severity::High, file.is_test_file),
                    category: FindingCategory::SqlInjection,
                    title: "Posible inyección SQL".into(),
                    description: format!(
                        "Concatenación/interpolación de strings en consulta SQL en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Usar queries parametrizadas / prepared statements. Nunca concatenar input del usuario en strings SQL.".into(),
                });
            }
        }
    }
}

// ── 2. Command Injection ────────────────────────────────────

fn scan_command_injection(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_subprocess_shell = Regex::new(r#"(?i)subprocess\.(run|Popen|call|check_output)\s*\([^)]*shell\s*=\s*True"#).unwrap();
    let py_os_system = Regex::new(r#"os\.system\s*\(\s*[a-zA-Z_]"#).unwrap();
    let py_os_popen = Regex::new(r#"os\.popen\s*\(\s*[a-zA-Z_]"#).unwrap();
    let py_eval = Regex::new(r#"\b(exec|eval)\s*\(\s*[a-zA-Z_]"#).unwrap();
    let js_child_proc = Regex::new(r#"\b(exec|execSync|spawn|spawnSync)\s*\(\s*(`[^`]*\$\{|[a-zA-Z_])"#).unwrap();
    let js_eval = Regex::new(r#"\beval\s*\(\s*[a-zA-Z_]"#).unwrap();
    let go_exec = Regex::new(r#"exec\.Command\s*\(\s*[a-zA-Z_]"#).unwrap();
    let rs_command = Regex::new(r#"Command::new\s*\(\s*[a-zA-Z_]"#).unwrap();

    for file in files {
        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            let matched = match file.ext.as_str() {
                "py" => {
                    py_subprocess_shell.is_match(line)
                        || py_os_system.is_match(line)
                        || py_os_popen.is_match(line)
                        || py_eval.is_match(line)
                }
                "js" | "ts" | "jsx" | "tsx" => {
                    js_child_proc.is_match(line) || js_eval.is_match(line)
                }
                "go" => go_exec.is_match(line),
                "rs" => rs_command.is_match(line),
                _ => false,
            };

            if matched {
                findings.push(SecurityFinding {
                    id: format!("cmdi-{}", findings.len()),
                    severity: adjust_severity(Severity::Critical, file.is_test_file),
                    category: FindingCategory::CommandInjection,
                    title: "Posible inyección de comandos".into(),
                    description: format!(
                        "Ejecución de comando con input variable en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "No pasar input del usuario a comandos shell. Usar arrays de argumentos en vez de shell=True. Validar y allowlist todos los inputs.".into(),
                });
            }
        }
    }
}

// ── 3. Path Traversal ───────────────────────────────────────

fn scan_path_traversal(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_open = Regex::new(r#"\bopen\s*\(\s*[a-zA-Z_]"#).unwrap();
    let py_send_file = Regex::new(r#"(?i)(send_file|send_from_directory|FileResponse)\s*\(\s*[a-zA-Z_]"#).unwrap();
    let js_fs_read = Regex::new(r#"fs\.(readFile|readFileSync|createReadStream)\s*\(\s*[a-zA-Z_]"#).unwrap();
    let js_send_file = Regex::new(r#"res\.sendFile\s*\(\s*[a-zA-Z_]"#).unwrap();

    for file in files {
        if !matches!(file.ext.as_str(), "py" | "js" | "ts" | "jsx" | "tsx") {
            continue;
        }
        let is_python = file.ext == "py";

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            let matched = if is_python {
                // Skip if line contains path validation
                let has_validation = line.contains("os.path.abspath")
                    || line.contains("pathlib")
                    || line.contains(".resolve()")
                    || line.contains("secure_filename");
                !has_validation && (py_open.is_match(line) || py_send_file.is_match(line))
                    && (line.contains("request") || line.contains("param") || line.contains("arg"))
            } else {
                let has_validation = line.contains("path.resolve")
                    || line.contains("path.normalize")
                    || line.contains("path.join");
                !has_validation
                    && (js_fs_read.is_match(line) || js_send_file.is_match(line))
                    && (line.contains("req.") || line.contains("params") || line.contains("query"))
            };

            if matched {
                findings.push(SecurityFinding {
                    id: format!("pathtr-{}", findings.len()),
                    severity: adjust_severity(Severity::High, file.is_test_file),
                    category: FindingCategory::PathTraversal,
                    title: "Posible path traversal".into(),
                    description: format!(
                        "Acceso a archivo con input variable sin validación en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Validar y resolver rutas de archivos. Usar path.resolve() y verificar que el resultado empiece con el directorio base. Nunca pasar input crudo a funciones del filesystem.".into(),
                });
            }
        }
    }
}

// ── 4. Insecure Deserialization ──────────────────────────────

fn scan_insecure_deserialization(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_pickle = Regex::new(r#"pickle\.(loads?|Unpickler)\s*\("#).unwrap();
    let py_yaml_unsafe = Regex::new(r#"yaml\.load\s*\([^)]*\)"#).unwrap();
    let py_yaml_safe = Regex::new(r#"yaml\.load\s*\([^)]*Loader\s*=\s*yaml\.SafeLoader"#).unwrap();
    let py_marshal = Regex::new(r#"marshal\.loads?\s*\("#).unwrap();
    let py_jsonpickle = Regex::new(r#"jsonpickle\.decode\s*\("#).unwrap();
    let js_unserialize = Regex::new(r#"\bunserialize\s*\(\s*[a-zA-Z_]"#).unwrap();

    for file in files {
        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            let matched = match file.ext.as_str() {
                "py" => {
                    if py_pickle.is_match(line) || py_marshal.is_match(line) || py_jsonpickle.is_match(line) {
                        true
                    } else if py_yaml_unsafe.is_match(line) && !py_yaml_safe.is_match(line) {
                        // yaml.load() without SafeLoader
                        !line.contains("safe_load")
                    } else {
                        false
                    }
                }
                "js" | "ts" | "jsx" | "tsx" => js_unserialize.is_match(line),
                _ => false,
            };

            if matched {
                findings.push(SecurityFinding {
                    id: format!("deser-{}", findings.len()),
                    severity: adjust_severity(Severity::High, file.is_test_file),
                    category: FindingCategory::InsecureDeserialization,
                    title: "Deserialización insegura".into(),
                    description: format!(
                        "Uso de deserialización insegura (pickle/yaml.load/marshal) en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Evitar pickle/marshal para datos no confiables. Usar yaml.safe_load() en vez de yaml.load(). Preferir JSON para serialización de datos externos.".into(),
                });
            }
        }
    }
}

// ── 5. Weak Cryptography ────────────────────────────────────

fn scan_weak_cryptography(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_weak_hash = Regex::new(r#"hashlib\.(md5|sha1)\s*\("#).unwrap();
    let py_weak_random = Regex::new(r#"\brandom\.(random|randint|choice|randrange)\s*\("#).unwrap();
    let py_weak_cipher = Regex::new(r#"(?i)(DES|RC4|Blowfish|RC2)"#).unwrap();
    let py_hardcoded_iv = Regex::new(r#"(?i)(iv|nonce)\s*=\s*b['"]\\x00"#).unwrap();
    let js_weak_hash = Regex::new(r#"createHash\s*\(\s*['"](?:md5|sha1)['"]\s*\)"#).unwrap();
    let js_math_random = Regex::new(r#"Math\.random\s*\("#).unwrap();
    let go_weak_hash = Regex::new(r#"(md5|sha1)\.New\s*\("#).unwrap();
    let rs_weak_crate = Regex::new(r#"use\s+(md5|sha1)"#).unwrap();

    let security_filename_words = ["password", "auth", "token", "secret", "key", "otp", "crypt", "hash", "sign", "verify"];

    for file in files {
        let rel_lower = file.rel_path.to_lowercase();
        let is_security_file = security_filename_words
            .iter()
            .any(|w| rel_lower.contains(w));

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            let matched = match file.ext.as_str() {
                "py" => {
                    if py_weak_hash.is_match(line) {
                        // Only flag if in security context or surrounding code suggests password use
                        let context = line.to_lowercase();
                        is_security_file
                            || context.contains("password")
                            || context.contains("hash")
                            || context.contains("sign")
                            || context.contains("verify")
                            || context.contains("token")
                    } else if py_weak_random.is_match(line) && is_security_file {
                        true
                    } else {
                        py_weak_cipher.is_match(line) && line.contains("(")
                            || py_hardcoded_iv.is_match(line)
                    }
                }
                "js" | "ts" | "jsx" | "tsx" => {
                    if js_weak_hash.is_match(line) {
                        let context = line.to_lowercase();
                        is_security_file
                            || context.contains("password")
                            || context.contains("hash")
                            || context.contains("sign")
                            || context.contains("verify")
                    } else {
                        js_math_random.is_match(line) && is_security_file
                    }
                }
                "go" => {
                    if go_weak_hash.is_match(line) {
                        is_security_file
                            || line.to_lowercase().contains("password")
                            || line.to_lowercase().contains("hash")
                    } else {
                        false
                    }
                }
                "rs" => rs_weak_crate.is_match(line) && is_security_file,
                _ => false,
            };

            if matched {
                let severity = if is_security_file {
                    Severity::High
                } else {
                    Severity::Medium
                };

                findings.push(SecurityFinding {
                    id: format!("crypto-{}", findings.len()),
                    severity: adjust_severity(severity, file.is_test_file),
                    category: FindingCategory::WeakCryptography,
                    title: "Criptografía débil".into(),
                    description: format!(
                        "Uso de algoritmo criptográfico débil o inseguro en {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Usar SHA-256+ para hashing. Usar bcrypt/argon2/scrypt para passwords. Usar crypto.randomBytes() o secrets.token_bytes() para aleatoriedad criptográfica.".into(),
                });
            }
        }
    }
}

// ── 6. XSS Detection ───────────────────────────────────────

fn scan_xss(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
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

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            // Check for string literal (not a variable)
            let has_literal_only = line.contains("innerHTML = \"")
                || line.contains("innerHTML = '")
                || line.contains("innerHTML = `");

            if inner_html.is_match(line) && !has_literal_only
                || outer_html.is_match(line)
                || doc_write.is_match(line)
                || insert_html.is_match(line)
                || eval_var.is_match(line)
                || new_function.is_match(line)
            {
                findings.push(SecurityFinding {
                    id: format!("xss-{}", findings.len()),
                    severity: adjust_severity(Severity::High, file.is_test_file),
                    category: FindingCategory::XssVulnerability,
                    title: "Posible XSS".into(),
                    description: format!(
                        "Asignación de HTML no sanitizado o eval() en {}:{}",
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
                        "Uso de dangerouslySetInnerHTML en {}:{} — React escapa por defecto, pero revisar",
                        file.rel_path,
                        i + 1
                    ),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Asegurar que el contenido está sanitizado con DOMPurify antes de usar dangerouslySetInnerHTML.".into(),
                });
            }
        }
    }
}

// ── 7. SSRF Detection ───────────────────────────────────────

fn scan_ssrf(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
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

// ── 8. Exposed Debug Endpoints ──────────────────────────────

fn scan_debug_endpoints(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let dangerous_paths = [
        "/debug", "/_debug", "/__debug", "/admin/debug",
        "/phpinfo", "/actuator", "/actuator/health", "/actuator/env",
        "/heapdump", "/.env", "/metrics",
    ];

    let py_route_re = Regex::new(r#"@(app|router)\.(get|post|route|api_route)\s*\(\s*['"]([^'"]+)['"]\s*"#).unwrap();
    let js_route_re = Regex::new(r#"(app|router)\.(get|post|use|all)\s*\(\s*['"]([^'"]+)['"]\s*"#).unwrap();

    for file in files {
        let route_re = match file.ext.as_str() {
            "py" => &py_route_re,
            "js" | "ts" => &js_route_re,
            _ => continue,
        };

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            if let Some(caps) = route_re.captures(line) {
                if let Some(path_match) = caps.get(3) {
                    let route_path = path_match.as_str().to_lowercase();
                    if dangerous_paths.iter().any(|p| route_path == *p || route_path.starts_with(&format!("{}/", p))) {
                        findings.push(SecurityFinding {
                            id: format!("debug-ep-{}", findings.len()),
                            severity: adjust_severity(Severity::Medium, file.is_test_file),
                            category: FindingCategory::ExposedDebugEndpoint,
                            title: format!("Endpoint de debug expuesto: {}", path_match.as_str()),
                            description: format!(
                                "Ruta de debug/diagnóstico expuesta en {}:{}",
                                file.rel_path,
                                i + 1
                            ),
                            file_path: Some(file.rel_path.clone()),
                            line_number: Some((i + 1) as u32),
                            remediation: "Eliminar o proteger endpoints de debug antes de deploy a producción. Usar middleware de autenticación y guards de entorno.".into(),
                        });
                    }
                }
            }
        }
    }
}

// ── 9. Secrets in Git History ───────────────────────────────

fn scan_git_history(project_path: &Path, findings: &mut Vec<SecurityFinding>) {
    if !project_path.join(".git").exists() {
        return;
    }

    let result = Command::new("git")
        .args([
            "-C",
            &project_path.to_string_lossy(),
            "log",
            "--all",
            "--oneline",
            "--diff-filter=D",
            "-S", "password",
            "--pickaxe-regex",
            "--format=%h %s",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let child = match result {
        Ok(c) => c,
        Err(_) => return,
    };

    let output = wait_git(child);
    let mut commits_found = Vec::new();

    if let Some(ref out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !stdout.trim().is_empty() {
            for line in stdout.lines().take(5) {
                commits_found.push(line.to_string());
            }
        }
    }

    // Also search for other sensitive keywords
    for keyword in &["secret", "AKIA", "api_key", "token"] {
        let result2 = Command::new("git")
            .args([
                "-C",
                &project_path.to_string_lossy(),
                "log",
                "--all",
                "--oneline",
                "--diff-filter=D",
                "-S", keyword,
                "--pickaxe-regex",
                "--format=%h %s",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        if let Ok(child2) = result2 {
            if let Some(out2) = wait_git(child2) {
                let stdout = String::from_utf8_lossy(&out2.stdout);
                for line in stdout.lines().take(3) {
                    let l = line.to_string();
                    if !commits_found.contains(&l) {
                        commits_found.push(l);
                    }
                }
            }
        }
    }

    if !commits_found.is_empty() {
        let commit_list = commits_found
            .iter()
            .take(10)
            .map(|c| format!("  {}", c))
            .collect::<Vec<_>>()
            .join("\n");

        findings.push(SecurityFinding {
            id: "git-history-secrets-0".into(),
            severity: Severity::High,
            category: FindingCategory::SecretInGitHistory,
            title: "Posibles secrets en historial Git".into(),
            description: format!(
                "Se encontraron {} commits con strings sensibles eliminados del código actual:\n{}",
                commits_found.len(),
                commit_list
            ),
            file_path: None,
            line_number: None,
            remediation: "Usar git filter-branch o BFG Repo Cleaner para purgar secrets del historial. Rotar todas las credenciales expuestas inmediatamente.".into(),
        });
    }
}

fn wait_git(mut child: std::process::Child) -> Option<std::process::Output> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => return None,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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
