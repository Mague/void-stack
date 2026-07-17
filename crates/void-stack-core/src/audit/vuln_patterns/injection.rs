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
                findings.push(SecurityFinding::new(
                    format!("sqli-{}", findings.len()),
                    adjust_severity(Severity::High, file.is_test_file),
                    FindingCategory::SqlInjection,
                    "Possible SQL injection".into(),
                    format!(
                        "String concatenation/interpolation in a SQL query in {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Use parameterized queries / prepared statements. Never concatenate user input into SQL strings.".into(),
                ));
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
                findings.push(SecurityFinding::new(
                    format!("cmdi-{}", findings.len()),
                    adjust_severity(Severity::Critical, file.is_test_file),
                    FindingCategory::CommandInjection,
                    "Possible command injection".into(),
                    format!(
                        "Command execution with variable input in {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Do not pass user input to shell commands. Use argument arrays instead of shell=True. Validate and allowlist all inputs.".into(),
                ));
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
                findings.push(SecurityFinding::new(
                    format!("pathtr-{}", findings.len()),
                    adjust_severity(Severity::High, file.is_test_file),
                    FindingCategory::PathTraversal,
                    "Possible path traversal".into(),
                    format!(
                        "File access with unvalidated variable input in {}:{}",
                        file.rel_path,
                        i + 1
                    ),
                    Some(file.rel_path.clone()),
                    Some((i + 1) as u32),
                    "Validate and resolve file paths. Use path.resolve() and verify the result starts with the base directory. Never pass raw input to filesystem functions.".into(),
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

    // ── SQL injection ──────────────────────────────────────────

    #[test]
    fn test_sql_injection_python_fstring() {
        let file = make_file(
            "app.py",
            "py",
            r#"db.execute(f"SELECT * FROM users WHERE id = {user_id}")"#,
        );
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1, "f-string SQL should be flagged");
        assert!(matches!(
            findings[0].category,
            FindingCategory::SqlInjection
        ));
        assert!(matches!(findings[0].severity, Severity::High));
        assert_eq!(findings[0].line_number, Some(1));
    }

    #[test]
    fn test_sql_injection_python_execute_concat() {
        let file = make_file(
            "dao.py",
            "py",
            r#"cursor.execute("SELECT * FROM users WHERE id=" + user_id)"#,
        );
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert_eq!(
            findings.len(),
            1,
            "string concat in execute() should be flagged"
        );
    }

    #[test]
    fn test_sql_injection_python_raw_query() {
        let file = make_file("models.py", "py", "rows = User.objects.raw(query)");
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert_eq!(
            findings.len(),
            1,
            ".raw() with a variable should be flagged"
        );
    }

    #[test]
    fn test_sql_injection_js_template_literal() {
        let file = make_file(
            "api.ts",
            "ts",
            r#"const rows = await db.query(`SELECT * FROM users WHERE name = ${name}`)"#,
        );
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1, "template literal SQL should be flagged");
    }

    #[test]
    fn test_sql_injection_js_query_concat() {
        let file = make_file(
            "db.js",
            "js",
            r#"db.query("SELECT * FROM t WHERE id=" + id)"#,
        );
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert_eq!(
            findings.len(),
            1,
            "string concat in query() should be flagged"
        );
    }

    #[test]
    fn test_sql_injection_parameterized_query_ok() {
        // Parameterized queries pass values separately — no concat/format/interp.
        let file = make_file("dao.py", "py", "cursor.execute(sql, params)");
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert!(
            findings.is_empty(),
            "parameterized query must not be flagged"
        );
    }

    #[test]
    fn test_sql_injection_skips_comments() {
        let file = make_file(
            "app.py",
            "py",
            r##"# db.execute(f"SELECT * FROM t WHERE id = {x}")"##,
        );
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert!(findings.is_empty(), "commented-out SQL must not be flagged");
    }

    #[test]
    fn test_sql_injection_ignores_unrelated_extensions() {
        // The SQL scanner only inspects Python and JS/TS files.
        let file = make_file(
            "main.go",
            "go",
            r#"db.Query("SELECT * FROM t WHERE id=" + id)"#,
        );
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_sql_injection_severity_reduced_in_test_file() {
        let file = FileInfo {
            rel_path: "tests/test_dao.py".into(),
            content: r#"db.execute(f"SELECT * FROM t WHERE id = {x}")"#.into(),
            ext: "py".into(),
            is_test_file: true,
        };
        let mut findings = Vec::new();
        scan_sql_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        // High is downgraded to Medium inside test files.
        assert!(matches!(findings[0].severity, Severity::Medium));
    }

    // ── Command injection ──────────────────────────────────────

    #[test]
    fn test_command_injection_python_subprocess_shell_true() {
        let file = make_file("runner.py", "py", "subprocess.run(cmd, shell=True)");
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0].category,
            FindingCategory::CommandInjection
        ));
        assert!(matches!(findings[0].severity, Severity::Critical));
    }

    #[test]
    fn test_command_injection_python_os_system_variable() {
        let file = make_file("runner.py", "py", "os.system(user_cmd)");
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_command_injection_python_os_system_literal_ok() {
        // A hardcoded command string is not injectable — regex requires an identifier.
        let file = make_file("runner.py", "py", r#"os.system("ls -la")"#);
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert!(findings.is_empty(), "literal command must not be flagged");
    }

    #[test]
    fn test_command_injection_python_eval_variable() {
        let file = make_file("plugin.py", "py", "result = eval(expression)");
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_command_injection_js_exec_template() {
        let file = make_file("build.js", "js", "exec(`rm -rf ${target}`)");
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_command_injection_go_sprintf() {
        let file = make_file(
            "main.go",
            "go",
            r#"cmd := exec.Command(fmt.Sprintf("ls %s", dir))"#,
        );
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_command_injection_rust_format() {
        let file = make_file(
            "src/exec.rs",
            "rs",
            r#"let child = Command::new(format!("{}", tool));"#,
        );
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_command_injection_rust_literal_ok() {
        // A fixed binary name cannot be injected.
        let file = make_file("src/exec.rs", "rs", r#"let child = Command::new("git");"#);
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_command_injection_skips_comments() {
        let file = make_file("handler.js", "js", "// exec(userCmd)");
        let mut findings = Vec::new();
        scan_command_injection(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    // ── Path traversal ─────────────────────────────────────────

    #[test]
    fn test_path_traversal_python_open_with_request_input() {
        let file = make_file("views.py", "py", r#"f = open(request.args.get("path"))"#);
        let mut findings = Vec::new();
        scan_path_traversal(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0].category,
            FindingCategory::PathTraversal
        ));
    }

    #[test]
    fn test_path_traversal_python_validated_ok() {
        // os.path.abspath on the same line counts as validation.
        let file = make_file(
            "views.py",
            "py",
            r#"f = open(os.path.abspath(request.args.get("path")))"#,
        );
        let mut findings = Vec::new();
        scan_path_traversal(&[file], &mut findings);
        assert!(findings.is_empty(), "validated path must not be flagged");
    }

    #[test]
    fn test_path_traversal_js_fs_read_with_req_input() {
        let file = make_file("server.js", "js", "fs.readFile(req.query.path, callback)");
        let mut findings = Vec::new();
        scan_path_traversal(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_path_traversal_js_path_join_ok() {
        // path.join on the same line counts as validation.
        let file = make_file(
            "server.js",
            "js",
            "fs.readFile(path.join(base, req.params.name), callback)",
        );
        let mut findings = Vec::new();
        scan_path_traversal(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_path_traversal_without_user_input_ok() {
        // File access with a plain local variable and no request/param/arg
        // keywords is not considered traversal.
        let file = make_file("loader.py", "py", "data = open(filename).read()");
        let mut findings = Vec::new();
        scan_path_traversal(&[file], &mut findings);
        assert!(findings.is_empty());
    }
}
