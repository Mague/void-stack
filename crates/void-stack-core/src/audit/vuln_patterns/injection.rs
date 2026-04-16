//! SQL injection, command injection, and path traversal pattern detectors.

use std::sync::OnceLock;

use regex::Regex;

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{FileInfo, adjust_severity, is_comment};

// ── Static regex helpers ────────────────────────────────────────

fn py_fstring_sql_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)f["'][^"']*\b(SELECT|INSERT|UPDATE|DELETE|WHERE)\b[^"']*\{"#)
            .expect("hardcoded regex")
    })
}

fn py_execute_concat_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\.execute\s*\([^)]*(\+|\.format\s*\(|%\s*[(\w])"#)
            .expect("hardcoded regex")
    })
}

fn py_raw_sql_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)\.raw\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn js_template_sql_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)`[^`]*\b(SELECT|INSERT|UPDATE|DELETE|WHERE)\b[^`]*\$\{"#)
            .expect("hardcoded regex")
    })
}

fn js_query_concat_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)\.(query|execute)\s*\([^)]*\+"#).expect("hardcoded regex"))
}

fn py_subprocess_shell_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)subprocess\.(run|Popen|call|check_output)\s*\([^)]*shell\s*=\s*True"#)
            .expect("hardcoded regex")
    })
}

fn py_os_system_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"os\.system\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn py_os_popen_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"os\.popen\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn py_eval_exec_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\b(exec|eval)\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn js_child_proc_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"\b(exec|execSync|spawn|spawnSync)\s*\(\s*(`[^`]*\$\{|[a-zA-Z_])"#)
            .expect("hardcoded regex")
    })
}

fn js_eval_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\beval\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn go_exec_command_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"exec\.Command\s*\(\s*(fmt\.Sprintf|[a-zA-Z_]+\s*\+)"#)
            .expect("hardcoded regex")
    })
}

fn rs_command_unsafe_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"Command::new\s*\(\s*(&?format!|&?\w+\s*\+)"#).expect("hardcoded regex")
    })
}

fn py_open_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\bopen\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

fn py_send_file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(send_file|send_from_directory|FileResponse)\s*\(\s*[a-zA-Z_]"#)
            .expect("hardcoded regex")
    })
}

fn js_fs_read_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"fs\.(readFile|readFileSync|createReadStream)\s*\(\s*[a-zA-Z_]"#)
            .expect("hardcoded regex")
    })
}

fn js_send_file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"res\.sendFile\s*\(\s*[a-zA-Z_]"#).expect("hardcoded regex"))
}

// ── SQL Injection ────────────────────────────────────────────

pub(crate) fn scan_sql_injection(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_fstring = py_fstring_sql_re();
    let py_execute_concat = py_execute_concat_re();
    let py_raw = py_raw_sql_re();
    let js_template_sql = js_template_sql_re();
    let js_query_concat = js_query_concat_re();

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
                    title: "Posible inyecci\u{00f3}n SQL".into(),
                    description: format!(
                        "Concatenaci\u{00f3}n/interpolaci\u{00f3}n de strings en consulta SQL en {}:{}",
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

// ── Command Injection ────────────────────────────────────────

pub(crate) fn scan_command_injection(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_subprocess_shell = py_subprocess_shell_re();
    let py_os_system = py_os_system_re();
    let py_os_popen = py_os_popen_re();
    let py_eval = py_eval_exec_re();
    let js_child_proc = js_child_proc_re();
    let js_eval = js_eval_re();
    let go_exec = go_exec_command_re();
    let rs_command_unsafe = rs_command_unsafe_re();

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
                "rs" => rs_command_unsafe.is_match(line),
                _ => false,
            };

            if matched {
                findings.push(SecurityFinding {
                    id: format!("cmdi-{}", findings.len()),
                    severity: adjust_severity(Severity::Critical, file.is_test_file),
                    category: FindingCategory::CommandInjection,
                    title: "Posible inyecci\u{00f3}n de comandos".into(),
                    description: format!(
                        "Ejecuci\u{00f3}n de comando con input variable en {}:{}",
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

// ── Path Traversal ───────────────────────────────────────────

pub(crate) fn scan_path_traversal(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let py_open = py_open_re();
    let py_send_file = py_send_file_re();
    let js_fs_read = js_fs_read_re();
    let js_send_file = js_send_file_re();

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
                let has_validation = line.contains("os.path.abspath")
                    || line.contains("pathlib")
                    || line.contains(".resolve()")
                    || line.contains("secure_filename");
                !has_validation
                    && (py_open.is_match(line) || py_send_file.is_match(line))
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
                        "Acceso a archivo con input variable sin validaci\u{00f3}n en {}:{}",
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
