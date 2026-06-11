//! Security audit module — scans projects for vulnerabilities, secrets, and insecure configs.

pub mod config_check;
pub mod context;
pub mod deps;
pub mod enrichment;
pub mod findings;
pub mod secrets;
pub mod suppress;
pub mod vuln_patterns;

use std::path::Path;
use std::time::Instant;

pub use findings::{
    AuditResult, AuditSummary, FindingCategory, ScanStats, SecurityFinding, Severity,
};

/// Run a full security audit on a project.
/// Combines dependency scanning, secret detection, and config checks.
///
/// `result.scan_stats` reports how much work was actually done:
/// `files_scanned == 0` means the path resolved to nothing scannable and the
/// "clean" result must not be interpreted as project health.
pub fn audit_project(project_name: &str, project_path: &Path) -> AuditResult {
    let mut result = AuditResult::new(project_name, &project_path.to_string_lossy());

    result.scan_stats.files_scanned = count_scannable_files(project_path);
    result.scan_stats.rules_executed =
        (secrets::rule_count() + config_check::rule_count() + vuln_patterns::rule_count() + 1)
            as u32; // +1: dependency vulnerability scan

    // 1. Scan for hardcoded secrets (fast, file-based)
    let t = Instant::now();
    let secret_findings = secrets::scan_secrets(project_path);
    record_phase(&mut result, "secrets", t);
    for f in secret_findings {
        result.add_finding(f);
    }

    // 2. Scan for insecure configurations
    let t = Instant::now();
    let config_findings = config_check::scan_insecure_configs(project_path);
    record_phase(&mut result, "configs", t);
    for f in config_findings {
        result.add_finding(f);
    }

    // 3. Dependency vulnerability scanning (may be slow — runs external tools)
    let t = Instant::now();
    let dep_findings = deps::scan_dependency_vulnerabilities(project_path);
    record_phase(&mut result, "dependencies", t);
    for f in dep_findings {
        result.add_finding(f);
    }

    // 4. Code vulnerability patterns (SQL injection, command injection, XSS, etc.)
    let t = Instant::now();
    let vuln_findings = vuln_patterns::scan_vuln_patterns(project_path);
    record_phase(&mut result, "vuln_patterns", t);
    for f in vuln_findings {
        result.add_finding(f);
    }

    // Enrich findings with syntactic context + adjust severity
    let all_findings = std::mem::take(&mut result.findings);
    let enriched = enrichment::enrich_findings(all_findings, project_path);

    // Apply suppression rules (.void-audit-ignore + inline directives)
    let (kept, suppressed_count) = suppress::filter_suppressed(enriched, project_path);

    // Reset summary and recount from filtered findings only
    result.summary = AuditSummary::default();
    result.findings = Vec::new();
    for f in kept {
        result.add_finding(f);
    }
    result.suppressed = suppressed_count as u32;

    // Sort findings by severity (critical first)
    result.findings.sort_by_key(|f| f.severity);

    // Compute risk score
    result.compute_risk_score();

    result
}

fn record_phase(result: &mut AuditResult, name: &str, started: Instant) {
    result
        .scan_stats
        .phase_timings
        .push((name.to_string(), started.elapsed().as_millis() as u64));
}

/// Count the source files the scanners would consider, using the same
/// extension and skip-dir filters as the secret scanner. Metadata-only walk
/// (no file reads), bounded to the same depth as the scanners.
fn count_scannable_files(root: &Path) -> u32 {
    fn walk(dir: &Path, depth: u32, count: &mut u32) {
        if depth > 6 {
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if path.is_dir() {
                if secrets::SKIP_DIRS
                    .iter()
                    .any(|s| name.eq_ignore_ascii_case(s))
                {
                    continue;
                }
                walk(&path, depth + 1, count);
                continue;
            }
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if secrets::SCANNABLE_EXTENSIONS.iter().any(|e| ext == *e)
                || name == "dockerfile"
                || name.starts_with("docker-compose")
            {
                *count += 1;
            }
        }
    }

    let mut count = 0;
    walk(root, 0, &mut count);
    count
}

/// Generate a markdown report from audit results.
pub fn generate_report(result: &AuditResult) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Security Audit: {}\n\n", result.project_name));
    md.push_str(&format!("**Date:** {}\n\n", result.timestamp));

    // Summary
    md.push_str("## Summary\n\n");
    md.push_str(&format!(
        "| Severity | Count |\n|----------|-------|\n| 🔴 Critical | {} |\n| 🟠 High | {} |\n| 🟡 Medium | {} |\n| 🔵 Low | {} |\n| ℹ️ Info | {} |\n| **Total** | **{}** |\n\n",
        result.summary.critical,
        result.summary.high,
        result.summary.medium,
        result.summary.low,
        result.summary.info,
        result.summary.total,
    ));

    md.push_str(&format!(
        "**Risk Score:** {:.0}/100\n\n",
        result.summary.risk_score
    ));
    if result.suppressed > 0 {
        md.push_str(&format!(
            "**Suppressed:** {} (via .void-audit-ignore)\n\n",
            result.suppressed
        ));
    }

    if result.findings.is_empty() {
        md.push_str("✅ No security findings.\n");
        return md;
    }

    // Separate findings by section
    let vuln_categories = [
        "SQL injection",
        "Command injection",
        "Path traversal",
        "Insecure deserialization",
        "Weak cryptography",
        "Cross-Site Scripting (XSS)",
        "Server-Side Request Forgery (SSRF)",
        "Exposed debug endpoint",
        "Secret in Git history",
    ];

    let (vuln_findings, other_findings): (Vec<_>, Vec<_>) = result
        .findings
        .iter()
        .partition(|f| vuln_categories.contains(&f.category.to_string().as_str()));

    if !other_findings.is_empty() {
        md.push_str("## Findings — Secrets, Configs and Dependencies\n\n");
        for finding in &other_findings {
            write_finding(&mut md, finding);
        }
    }

    if !vuln_findings.is_empty() {
        md.push_str("## Code Vulnerability Patterns\n\n");
        for finding in &vuln_findings {
            write_finding(&mut md, finding);
        }
    }

    md
}

fn write_finding(md: &mut String, finding: &SecurityFinding) {
    let effective = finding.adjusted_severity;
    let icon = match effective {
        Severity::Critical => "🔴",
        Severity::High => "🟠",
        Severity::Medium => "🟡",
        Severity::Low => "🔵",
        Severity::Info => "ℹ️",
    };

    md.push_str(&format!(
        "### {} [{}] {}\n\n",
        icon, effective, finding.title
    ));
    md.push_str(&format!("**Category:** {}\n\n", finding.category));
    md.push_str(&format!("{}\n\n", finding.description));

    if let Some(ref path) = finding.file_path {
        if let Some(line) = finding.line_number {
            md.push_str(&format!("**File:** `{}:{}`\n\n", path, line));
        } else {
            md.push_str(&format!("**File:** `{}`\n\n", path));
        }
    }

    md.push_str(&format!("**Remediation:** {}\n\n", finding.remediation));
    md.push_str("---\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_audit_empty_project() {
        let dir = tempdir().unwrap();
        let result = audit_project("test", dir.path());
        assert_eq!(result.findings.len(), 0);
        assert_eq!(result.summary.risk_score, 0.0);
        // Nothing scannable — the stats must say so explicitly.
        assert_eq!(result.scan_stats.files_scanned, 0);
    }

    #[test]
    fn test_audit_nonexistent_path_reports_zero_scanned() {
        let result = audit_project("ghost", Path::new("/definitely/not/a/real/path"));
        assert_eq!(result.findings.len(), 0);
        assert_eq!(result.scan_stats.files_scanned, 0);
    }

    #[test]
    fn test_scan_stats_populated_on_seeded_project() {
        let dir = tempdir().unwrap();
        let key = format!("sk_{}_abc123def456ghi789jkl012mno345pqr678", "live");
        fs::write(
            dir.path().join("config.py"),
            format!(r#"API_KEY = "{}""#, key),
        )
        .unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        let result = audit_project("seeded", dir.path());
        assert_eq!(result.scan_stats.files_scanned, 2);
        assert!(result.scan_stats.rules_executed > 0);
        // All four audit phases must report a timing.
        let phases: Vec<&str> = result
            .scan_stats
            .phase_timings
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(
            phases,
            vec!["secrets", "configs", "dependencies", "vuln_patterns"]
        );
        // And the seeded finding must be detected.
        assert!(!result.findings.is_empty());
    }

    #[test]
    fn test_detect_hardcoded_api_key() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("config.py");
        let key = format!("sk_{}_abc123def456ghi789jkl012mno345pqr678", "live");
        fs::write(&file, format!(r#"API_KEY = "{}""#, key)).unwrap();
        let result = audit_project("test", dir.path());
        assert!(!result.findings.is_empty());
        assert!(
            result
                .findings
                .iter()
                .any(|f| matches!(f.category, FindingCategory::HardcodedSecret))
        );
    }

    #[test]
    fn test_detect_debug_mode() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("settings.py");
        fs::write(&file, "DEBUG = True\nALLOWED_HOSTS = ['*']").unwrap();
        let result = audit_project("test", dir.path());
        assert!(
            result
                .findings
                .iter()
                .any(|f| matches!(f.category, FindingCategory::DebugEnabled))
        );
    }

    #[test]
    fn test_risk_score_calculation() {
        let mut result = AuditResult::new("test", "/tmp/test");
        result.add_finding(SecurityFinding::new(
            "test-1".into(),
            Severity::Critical,
            FindingCategory::HardcodedSecret,
            "Test".into(),
            "Test".into(),
            None,
            None,
            "Fix it".into(),
        ));
        result.compute_risk_score();
        // New contextual scoring: Critical + Heuristic confidence = 10 pts
        // (was 40 under the old flat formula).
        assert_eq!(result.summary.risk_score, 10.0);
    }

    #[test]
    fn test_generate_report() {
        let result = AuditResult::new("my-app", "/projects/my-app");
        let report = generate_report(&result);
        assert!(report.contains("Security Audit: my-app"));
        assert!(report.contains("No security findings"));
    }
}
