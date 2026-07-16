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
                    findings.push(SecurityFinding::new(
                            format!("debug-ep-{}", findings.len()),
                            adjust_severity(Severity::Medium, file.is_test_file),
                            FindingCategory::ExposedDebugEndpoint,
                            format!("Exposed debug endpoint: {}", path_match.as_str()),
                            format!(
                                "Debug/diagnostics route exposed in {}:{}",
                                file.rel_path,
                                i + 1
                            ),
                            Some(file.rel_path.clone()),
                            Some((i + 1) as u32),
                            "Remove or protect debug endpoints before deploying to production. Use authentication middleware and environment guards.".into(),
                        ));
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

        findings.push(SecurityFinding::new(
            "git-history-secrets-0".into(),
            Severity::High,
            FindingCategory::SecretInGitHistory,
            "Possible secrets in Git history".into(),
            format!(
                "Found {} commits with sensitive strings removed from the current code:\n{}",
                commits_found.len(),
                commit_list
            ),
            None,
            None,
            "Use git filter-branch or BFG Repo Cleaner to purge secrets from history. Rotate all exposed credentials immediately.".into(),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn make_file(path: &str, ext: &str, content: &str) -> FileInfo {
        FileInfo {
            rel_path: path.into(),
            content: content.into(),
            ext: ext.into(),
            is_test_file: false,
        }
    }

    // ── Debug endpoints ────────────────────────────────────────

    #[test]
    fn test_debug_endpoint_python_route() {
        let file = make_file("routes.py", "py", r#"@app.get("/debug")"#);
        let mut findings = Vec::new();
        scan_debug_endpoints(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(matches!(
            findings[0].category,
            FindingCategory::ExposedDebugEndpoint
        ));
        assert!(findings[0].title.contains("/debug"));
    }

    #[test]
    fn test_debug_endpoint_python_subpath() {
        // Subpaths under a dangerous prefix also count.
        let file = make_file("routes.py", "py", r#"@router.route("/debug/vars")"#);
        let mut findings = Vec::new();
        scan_debug_endpoints(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_debug_endpoint_js_actuator() {
        let file = make_file(
            "server.ts",
            "ts",
            "app.get('/actuator/env', (req, res) => res.json(process.env))",
        );
        let mut findings = Vec::new();
        scan_debug_endpoints(&[file], &mut findings);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("/actuator/env"));
    }

    #[test]
    fn test_debug_endpoint_safe_route_ok() {
        let file = make_file("routes.py", "py", r#"@app.get("/users")"#);
        let mut findings = Vec::new();
        scan_debug_endpoints(&[file], &mut findings);
        assert!(findings.is_empty(), "normal routes must not be flagged");
    }

    #[test]
    fn test_debug_endpoint_skips_comments() {
        let file = make_file("routes.py", "py", r##"# @app.get("/debug")"##);
        let mut findings = Vec::new();
        scan_debug_endpoints(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_debug_endpoint_ignores_other_extensions() {
        // Only Python and JS/TS files carry route declarations this scanner knows.
        let file = make_file("main.go", "go", r#"app.get("/debug")"#);
        let mut findings = Vec::new();
        scan_debug_endpoints(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    // ── False-positive commit filter ───────────────────────────

    #[test]
    fn test_false_positive_commit_refactor_of_security_code() {
        // Two fp keywords ("refactor" + "audit") mark this as a code move.
        assert!(is_false_positive_commit("abc1234 refactor audit module"));
    }

    #[test]
    fn test_false_positive_commit_detection_pattern() {
        // Mentions of detection patterns/scanners are always filtered.
        assert!(is_false_positive_commit("abc1234 add new detection rules"));
        assert!(is_false_positive_commit("abc1234 improve scanner output"));
    }

    #[test]
    fn test_real_secret_commit_not_filtered() {
        assert!(!is_false_positive_commit(
            "abc1234 oops committed prod db credentials"
        ));
    }

    // ── Git history scan ───────────────────────────────────────

    /// Run a git command in `dir`, panicking on failure so test setup errors
    /// are visible.
    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .expect("git must be available for this test");
        assert!(status.success(), "git {:?} failed", args);
    }

    #[test]
    fn test_git_history_non_repo_returns_nothing() {
        let dir = tempdir().unwrap();
        let mut findings = Vec::new();
        scan_git_history(dir.path(), &mut findings);
        assert!(
            findings.is_empty(),
            "a directory without .git must be a no-op"
        );
    }

    #[test]
    fn test_git_history_detects_removed_secret() {
        // Build a minimal repo where a file containing "password" was
        // committed and later deleted — exactly what the pickaxe query finds.
        let dir = tempdir().unwrap();
        let path = dir.path();
        git(path, &["init", "-q"]);
        git(path, &["config", "user.email", "test@example.com"]);
        git(path, &["config", "user.name", "Test"]);
        git(path, &["config", "commit.gpgsign", "false"]);

        fs::write(path.join("creds.py"), "password = \"hunter2\"\n").unwrap();
        git(path, &["add", "creds.py"]);
        git(path, &["commit", "-q", "-m", "add creds file"]);
        git(path, &["rm", "-q", "creds.py"]);
        git(path, &["commit", "-q", "-m", "delete old creds file"]);

        let mut findings = Vec::new();
        scan_git_history(path, &mut findings);
        assert_eq!(
            findings.len(),
            1,
            "deleted secret should produce one finding"
        );
        assert!(matches!(
            findings[0].category,
            FindingCategory::SecretInGitHistory
        ));
        assert!(matches!(findings[0].severity, Severity::High));
        assert!(findings[0].description.contains("delete old creds file"));
    }

    #[test]
    fn test_git_history_clean_repo_ok() {
        // A repo with no sensitive strings ever committed yields no findings.
        let dir = tempdir().unwrap();
        let path = dir.path();
        git(path, &["init", "-q"]);
        git(path, &["config", "user.email", "test@example.com"]);
        git(path, &["config", "user.name", "Test"]);
        git(path, &["config", "commit.gpgsign", "false"]);

        fs::write(path.join("readme.txt"), "hello world\n").unwrap();
        git(path, &["add", "readme.txt"]);
        git(path, &["commit", "-q", "-m", "initial commit"]);

        let mut findings = Vec::new();
        scan_git_history(path, &mut findings);
        assert!(findings.is_empty());
    }
}
