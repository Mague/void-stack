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

pub use findings::{AuditResult, AuditSummary, FindingCategory, SecurityFinding, Severity};

/// Run a full security audit on a project.
/// Combines dependency scanning, secret detection, and config checks.
pub fn audit_project(project_name: &str, project_path: &Path) -> AuditResult {
    let mut result = AuditResult::new(project_name, &project_path.to_string_lossy());

    // 1. Scan for hardcoded secrets (fast, file-based)
    let secret_findings = secrets::scan_secrets(project_path);
    for f in secret_findings {
        result.add_finding(f);
    }

    // 2. Scan for insecure configurations
    let config_findings = config_check::scan_insecure_configs(project_path);
    for f in config_findings {
        result.add_finding(f);
    }

    // 3. Dependency vulnerability scanning (may be slow — runs external tools)
    let dep_findings = deps::scan_dependency_vulnerabilities(project_path);
    for f in dep_findings {
        result.add_finding(f);
    }

    // 4. Code vulnerability patterns (SQL injection, command injection, XSS, etc.)
    let vuln_findings = vuln_patterns::scan_vuln_patterns(project_path);
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
    result.findings.sort_by(|a, b| a.severity.cmp(&b.severity));

    // Compute risk score
    result.compute_risk_score();

    result
}

/// Generate a markdown report from audit results.
pub fn generate_report(result: &AuditResult) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Security Audit: {}\n\n", result.project_name));
    md.push_str(&format!("**Fecha:** {}\n\n", result.timestamp));

    // Summary
    md.push_str("## Resumen\n\n");
    md.push_str(&format!(
        "| Severidad | Cantidad |\n|-----------|----------|\n| 🔴 Critical | {} |\n| 🟠 High | {} |\n| 🟡 Medium | {} |\n| 🔵 Low | {} |\n| ℹ️ Info | {} |\n| **Total** | **{}** |\n\n",
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

    if result.findings.is_empty() {
        md.push_str("✅ No se encontraron hallazgos de seguridad.\n");
        return md;
    }

    // Separate findings by section
    let vuln_categories = [
        "Inyección SQL",
        "Inyección de comandos",
        "Path traversal",
        "Deserialización insegura",
        "Criptografía débil",
        "Cross-Site Scripting (XSS)",
        "Server-Side Request Forgery (SSRF)",
        "Endpoint de debug expuesto",
        "Secret en historial Git",
    ];

    let (vuln_findings, other_findings): (Vec<_>, Vec<_>) = result
        .findings
        .iter()
        .partition(|f| vuln_categories.contains(&f.category.to_string().as_str()));

    if !other_findings.is_empty() {
        md.push_str("## Hallazgos — Secrets, Configs y Dependencias\n\n");
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
    let icon = match finding.severity {
        Severity::Critical => "🔴",
        Severity::High => "🟠",
        Severity::Medium => "🟡",
        Severity::Low => "🔵",
        Severity::Info => "ℹ️",
    };

    md.push_str(&format!(
        "### {} [{}] {}\n\n",
        icon, finding.severity, finding.title
    ));
    md.push_str(&format!("**Categoría:** {}\n\n", finding.category));
    md.push_str(&format!("{}\n\n", finding.description));

    if let Some(ref path) = finding.file_path {
        if let Some(line) = finding.line_number {
            md.push_str(&format!("**Archivo:** `{}:{}`\n\n", path, line));
        } else {
            md.push_str(&format!("**Archivo:** `{}`\n\n", path));
        }
    }

    md.push_str(&format!("**Remediación:** {}\n\n", finding.remediation));
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
        assert!(report.contains("No se encontraron hallazgos"));
    }
}
