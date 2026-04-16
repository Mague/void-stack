//! Debug endpoint and insecure configuration pattern detectors.

use std::path::Path;
use std::process::Command;

use regex::Regex;
use std::sync::OnceLock;

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{FileInfo, adjust_severity, is_comment};
use crate::process_util::HideWindow;

fn py_route_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"@(app|router)\.(get|post|route|api_route)\s*\(\s*['"]([^'"]+)['"]\s*"#)
            .expect("hardcoded regex")
    })
}

fn js_route_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(app|router)\.(get|post|use|all)\s*\(\s*['"]([^'"]+)['"]\s*"#)
            .expect("hardcoded regex")
    })
}

// ── Exposed Debug Endpoints ──────────────────────────────────

pub(crate) fn scan_debug_endpoints(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    let dangerous_paths = [
        "/debug",
        "/_debug",
        "/__debug",
        "/admin/debug",
        "/phpinfo",
        "/actuator",
        "/actuator/health",
        "/actuator/env",
        "/heapdump",
        "/.env",
        "/metrics",
    ];

    for file in files {
        let route_re = match file.ext.as_str() {
            "py" => py_route_re(),
            "js" | "ts" => js_route_re(),
            _ => continue,
        };

        for (i, line) in file.content.lines().enumerate() {
            if is_comment(line) {
                continue;
            }

            if let Some(caps) = route_re.captures(line)
                && let Some(path_match) = caps.get(3)
            {
                let route_path = path_match.as_str().to_lowercase();
                if dangerous_paths
                    .iter()
                    .any(|p| route_path == *p || route_path.starts_with(&format!("{}/", p)))
                {
                    findings.push(SecurityFinding {
                            id: format!("debug-ep-{}", findings.len()),
                            severity: adjust_severity(Severity::Medium, file.is_test_file),
                            category: FindingCategory::ExposedDebugEndpoint,
                            title: format!("Endpoint de debug expuesto: {}", path_match.as_str()),
                            description: format!(
                                "Ruta de debug/diagn\u{00f3}stico expuesta en {}:{}",
                                file.rel_path,
                                i + 1
                            ),
                            file_path: Some(file.rel_path.clone()),
                            line_number: Some((i + 1) as u32),
                            remediation: "Eliminar o proteger endpoints de debug antes de deploy a producci\u{00f3}n. Usar middleware de autenticaci\u{00f3}n y guards de entorno.".into(),
                        });
                }
            }
        }
    }
}

// ── Secrets in Git History ───────────────────────────────────

pub(crate) fn scan_git_history(project_path: &Path, findings: &mut Vec<SecurityFinding>) {
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
            "-S",
            "password",
            "--pickaxe-regex",
            "--format=%h %s",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .hide_window()
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
                let l = line.to_string();
                if !is_false_positive_commit(&l) {
                    commits_found.push(l);
                }
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
                "-S",
                keyword,
                "--pickaxe-regex",
                "--format=%h %s",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .hide_window()
            .spawn();

        if let Ok(child2) = result2
            && let Some(out2) = wait_git(child2)
        {
            let stdout = String::from_utf8_lossy(&out2.stdout);
            for line in stdout.lines().take(3) {
                let l = line.to_string();
                if !commits_found.contains(&l) && !is_false_positive_commit(&l) {
                    commits_found.push(l);
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
                "Se encontraron {} commits con strings sensibles eliminados del c\u{00f3}digo actual:\n{}",
                commits_found.len(),
                commit_list
            ),
            file_path: None,
            line_number: None,
            remediation: "Usar git filter-branch o BFG Repo Cleaner para purgar secrets del historial. Rotar todas las credenciales expuestas inmediatamente.".into(),
        });
    }
}

/// Filter false-positive git history commits: refactors of security/audit code
/// that contain keywords like "password", "secret", "token" as detection patterns,
/// not as actual credentials.
fn is_false_positive_commit(line: &str) -> bool {
    let lower = line.to_lowercase();
    let fp_patterns = [
        "vuln_pattern",
        "vuln-pattern",
        "audit",
        "security",
        "secret", // commits about secrets detection code
        "refactor",
        "split",
        "extract",
        "move",
        "test",
        "spec",
        "lint",
        "clippy",
        "fmt",
        "doc",
        "readme",
        "changelog",
    ];
    // If the commit message suggests it's a refactor/split of security code, skip it
    let has_code_move = fp_patterns.iter().filter(|p| lower.contains(*p)).count() >= 2;
    if has_code_move {
        return true;
    }
    // Also skip if the commit message explicitly mentions detection pattern files
    if lower.contains("detection") || lower.contains("pattern") || lower.contains("scanner") {
        return true;
    }
    false
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
