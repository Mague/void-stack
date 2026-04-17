//! Dependency vulnerability scanning via npm audit, pip audit, cargo audit, etc.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::findings::{FindingCategory, SecurityFinding, Severity};

/// Run dependency vulnerability scans relevant to the project.
pub fn scan_dependency_vulnerabilities(project_path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    // npm audit
    if project_path.join("package-lock.json").exists() || project_path.join("package.json").exists()
    {
        findings.extend(scan_npm_audit(project_path));
    }

    // pip audit / safety
    if project_path.join("requirements.txt").exists()
        || project_path.join("pyproject.toml").exists()
    {
        findings.extend(scan_pip_audit(project_path));
    }

    // cargo audit
    if project_path.join("Cargo.lock").exists() {
        findings.extend(scan_cargo_audit(project_path));
    }

    // go vuln check
    if project_path.join("go.sum").exists() {
        findings.extend(scan_go_vuln(project_path));
    }

    // Scan subdirectories (1 level) for monorepos
    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let sub = entry.path();
            if !sub.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" {
                continue;
            }
            if sub.join("package-lock.json").exists() || sub.join("package.json").exists() {
                findings.extend(scan_npm_audit(&sub));
            }
            if sub.join("requirements.txt").exists() {
                findings.extend(scan_pip_audit(&sub));
            }
            if sub.join("Cargo.lock").exists() {
                findings.extend(scan_cargo_audit(&sub));
            }
            if sub.join("go.sum").exists() {
                findings.extend(scan_go_vuln(&sub));
            }
        }
    }

    findings
}

fn run_command_timeout(cmd: &str, args: &[&str], cwd: &Path, timeout_secs: u64) -> Option<String> {
    use crate::process_util::HideWindow;
    let child = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .hide_window()
        .spawn()
        .ok()?;

    let output = wait_with_timeout(child, Duration::from_secs(timeout_secs))?;
    // npm audit returns exit code 1 when vulns found, so we read stdout regardless
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if stdout.is_empty() && !stderr.is_empty() {
        Some(stderr)
    } else {
        Some(stdout)
    }
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<std::process::Output> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return None,
        }
    }
}

fn scan_npm_audit(dir: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let dir_label = dir.to_string_lossy().to_string();

    let output = match run_command_timeout("npm", &["audit", "--json"], dir, 30) {
        Some(o) => o,
        None => return findings,
    };

    // Parse npm audit JSON
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&output)
        && let Some(vulns) = json.get("vulnerabilities").and_then(|v| v.as_object())
    {
        for (pkg_name, vuln_info) in vulns {
            let severity_str = vuln_info
                .get("severity")
                .and_then(|s| s.as_str())
                .unwrap_or("low");
            let severity = match severity_str {
                "critical" => Severity::Critical,
                "high" => Severity::High,
                "moderate" => Severity::Medium,
                _ => Severity::Low,
            };

            let via = vuln_info
                .get("via")
                .and_then(|v| {
                    if let Some(arr) = v.as_array() {
                        arr.first().and_then(|item| {
                            if let Some(s) = item.as_str() {
                                Some(s.to_string())
                            } else {
                                item.get("title").and_then(|t| t.as_str()).map(String::from)
                            }
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "Vulnerability detected".into());

            let range = vuln_info
                .get("range")
                .and_then(|r| r.as_str())
                .unwrap_or("");

            findings.push(SecurityFinding::new(
                format!("npm-vuln-{}", pkg_name),
                severity,
                FindingCategory::DependencyVulnerability,
                format!("{} (npm)", pkg_name),
                format!("{} — versiones afectadas: {}", via, range),
                Some(format!("{}/package.json", dir_label)),
                None,
                format!("npm audit fix o actualizar {} manualmente", pkg_name),
            ));
        }
    }

    findings
}

fn scan_pip_audit(dir: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let dir_label = dir.to_string_lossy().to_string();

    // Try pip-audit first (modern), fallback to safety
    let output = run_command_timeout(
        "pip-audit",
        &["--format", "json", "-r", "requirements.txt"],
        dir,
        30,
    )
    .or_else(|| run_command_timeout("pip", &["audit", "--format", "json"], dir, 30));

    let output = match output {
        Some(o) => o,
        None => {
            // pip-audit not installed — report as info
            findings.push(SecurityFinding::new(
                "pip-audit-missing".into(),
                Severity::Info,
                FindingCategory::DependencyVulnerability,
                "pip-audit no instalado".into(),
                "No se pudo escanear vulnerabilidades en dependencias Python".into(),
                Some(format!("{}/requirements.txt", dir_label)),
                None,
                "pip install pip-audit".into(),
            ));
            return findings;
        }
    };

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&output)
        && let Some(deps) = json.get("dependencies").and_then(|d| d.as_array())
    {
        for dep in deps {
            let name = dep
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let version = dep.get("version").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(vulns) = dep.get("vulns").and_then(|v| v.as_array()) {
                for vuln in vulns {
                    let vuln_id = vuln
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("CVE-????");
                    let desc = vuln
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("Vulnerability found");
                    let fix_ver = vuln
                        .get("fix_versions")
                        .and_then(|f| f.as_array())
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("latest");

                    findings.push(SecurityFinding::new(
                        format!("pip-{}-{}", name, vuln_id),
                        Severity::High,
                        FindingCategory::DependencyVulnerability,
                        format!("{} {} ({})", name, version, vuln_id),
                        desc.to_string(),
                        Some(format!("{}/requirements.txt", dir_label)),
                        None,
                        format!("Actualizar {} a >= {}", name, fix_ver),
                    ));
                }
            }
        }
    }

    findings
}

fn scan_cargo_audit(dir: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let dir_label = dir.to_string_lossy().to_string();

    let output = match run_command_timeout("cargo", &["audit", "--json"], dir, 60) {
        Some(o) => o,
        None => {
            findings.push(SecurityFinding::new(
                "cargo-audit-missing".into(),
                Severity::Info,
                FindingCategory::DependencyVulnerability,
                "cargo-audit no instalado".into(),
                "No se pudo escanear vulnerabilidades en dependencias Rust".into(),
                Some(format!("{}/Cargo.lock", dir_label)),
                None,
                "cargo install cargo-audit".into(),
            ));
            return findings;
        }
    };

    // cargo audit --json outputs newline-delimited JSON objects
    for line in output.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line)
            && json.get("advisory").is_some()
        {
            let advisory = &json["advisory"];
            let id = advisory
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("RUSTSEC-????");
            let pkg = json
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let title = advisory
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("Security advisory");
            let desc = advisory
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("");

            let severity = if id.contains("RUSTSEC") {
                Severity::High
            } else {
                Severity::Medium
            };

            let patched = json
                .get("versions")
                .and_then(|v| v.get("patched"))
                .and_then(|p| p.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("latest");

            findings.push(SecurityFinding::new(
                format!("cargo-{}-{}", pkg, id),
                severity,
                FindingCategory::DependencyVulnerability,
                format!("{} ({})", pkg, id),
                format!("{}: {}", title, desc),
                Some(format!("{}/Cargo.lock", dir_label)),
                None,
                format!("Actualizar {} a {}", pkg, patched),
            ));
        }
    }

    findings
}

fn scan_go_vuln(dir: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let dir_label = dir.to_string_lossy().to_string();

    let output = match run_command_timeout("govulncheck", &["-json", "./..."], dir, 60) {
        Some(o) => o,
        None => {
            findings.push(SecurityFinding::new(
                "govulncheck-missing".into(),
                Severity::Info,
                FindingCategory::DependencyVulnerability,
                "govulncheck no instalado".into(),
                "No se pudo escanear vulnerabilidades en dependencias Go".into(),
                Some(format!("{}/go.sum", dir_label)),
                None,
                "go install golang.org/x/vuln/cmd/govulncheck@latest".into(),
            ));
            return findings;
        }
    };

    // govulncheck -json outputs JSON objects
    for line in output.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(finding) = json.get("finding")
        {
            let osv = finding
                .get("osv")
                .and_then(|o| o.as_str())
                .unwrap_or("GO-????");

            let trace = finding.get("trace").and_then(|t| t.as_array());
            let module = trace
                .and_then(|t| t.first())
                .and_then(|f| f.get("module"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");

            findings.push(SecurityFinding::new(
                format!("go-{}-{}", module, osv),
                Severity::High,
                FindingCategory::DependencyVulnerability,
                format!("{} ({})", module, osv),
                format!("Vulnerabilidad detectada en módulo Go: {}", module),
                Some(format!("{}/go.sum", dir_label)),
                None,
                format!("go get -u {} && go mod tidy", module),
            ));
        }
    }

    findings
}
