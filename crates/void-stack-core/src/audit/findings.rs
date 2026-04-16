use serde::{Deserialize, Serialize};
use std::fmt;

/// Severity level for security findings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Critical => write!(f, "critical"),
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
            Self::Info => write!(f, "info"),
        }
    }
}

/// Category of the security finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingCategory {
    DependencyVulnerability,
    HardcodedSecret,
    InsecureConfig,
    MissingSecurityHeader,
    DebugEnabled,
    WeakCrypto,
    PathTraversal,
    PermissivePermissions,
    SqlInjection,
    CommandInjection,
    InsecureDeserialization,
    WeakCryptography,
    XssVulnerability,
    Ssrf,
    ExposedDebugEndpoint,
    SecretInGitHistory,
    UnsafeErrorHandling,
}

impl fmt::Display for FindingCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DependencyVulnerability => write!(f, "Vulnerabilidad en dependencia"),
            Self::HardcodedSecret => write!(f, "Secret hardcodeado"),
            Self::InsecureConfig => write!(f, "Configuración insegura"),
            Self::MissingSecurityHeader => write!(f, "Header de seguridad faltante"),
            Self::DebugEnabled => write!(f, "Debug habilitado"),
            Self::WeakCrypto => write!(f, "Criptografía débil"),
            Self::PathTraversal => write!(f, "Path traversal"),
            Self::PermissivePermissions => write!(f, "Permisos permisivos"),
            Self::SqlInjection => write!(f, "Inyección SQL"),
            Self::CommandInjection => write!(f, "Inyección de comandos"),
            Self::InsecureDeserialization => write!(f, "Deserialización insegura"),
            Self::WeakCryptography => write!(f, "Criptografía débil"),
            Self::XssVulnerability => write!(f, "Cross-Site Scripting (XSS)"),
            Self::Ssrf => write!(f, "Server-Side Request Forgery (SSRF)"),
            Self::ExposedDebugEndpoint => write!(f, "Endpoint de debug expuesto"),
            Self::SecretInGitHistory => write!(f, "Secret en historial Git"),
            Self::UnsafeErrorHandling => write!(f, "Manejo de errores inseguro"),
        }
    }
}

/// A single security finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub id: String,
    pub severity: Severity,
    pub category: FindingCategory,
    pub title: String,
    pub description: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub remediation: String,
}

/// Summary counts by severity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditSummary {
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub info: u32,
    pub total: u32,
    pub risk_score: f32,
}

/// Result of a full security audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResult {
    pub project_name: String,
    pub project_path: String,
    pub timestamp: String,
    pub findings: Vec<SecurityFinding>,
    pub summary: AuditSummary,
    /// Number of findings suppressed by `.void-audit-ignore` or inline directives.
    #[serde(default)]
    pub suppressed: u32,
}

impl AuditResult {
    pub fn new(project_name: &str, project_path: &str) -> Self {
        Self {
            project_name: project_name.into(),
            project_path: project_path.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            findings: Vec::new(),
            summary: AuditSummary::default(),
            suppressed: 0,
        }
    }

    pub fn add_finding(&mut self, finding: SecurityFinding) {
        match finding.severity {
            Severity::Critical => self.summary.critical += 1,
            Severity::High => self.summary.high += 1,
            Severity::Medium => self.summary.medium += 1,
            Severity::Low => self.summary.low += 1,
            Severity::Info => self.summary.info += 1,
        }
        self.summary.total += 1;
        self.findings.push(finding);
    }

    pub fn compute_risk_score(&mut self) {
        // Weighted score: critical=40, high=20, medium=5, low=1
        let raw = (self.summary.critical as f32 * 40.0)
            + (self.summary.high as f32 * 20.0)
            + (self.summary.medium as f32 * 5.0)
            + (self.summary.low as f32 * 1.0);
        // Normalize to 0-100 (cap at 100)
        self.summary.risk_score = raw.min(100.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_finding(severity: Severity, category: FindingCategory) -> SecurityFinding {
        SecurityFinding {
            id: "test-1".into(),
            severity,
            category,
            title: "Test finding".into(),
            description: "Test".into(),
            file_path: Some("app.py".into()),
            line_number: Some(1),
            remediation: "Fix it".into(),
        }
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Critical), "critical");
        assert_eq!(format!("{}", Severity::High), "high");
        assert_eq!(format!("{}", Severity::Medium), "medium");
        assert_eq!(format!("{}", Severity::Low), "low");
        assert_eq!(format!("{}", Severity::Info), "info");
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical < Severity::High);
        assert!(Severity::High < Severity::Medium);
        assert!(Severity::Medium < Severity::Low);
        assert!(Severity::Low < Severity::Info);
    }

    #[test]
    fn test_finding_category_display() {
        assert_eq!(
            format!("{}", FindingCategory::SqlInjection),
            "Inyección SQL"
        );
        assert_eq!(
            format!("{}", FindingCategory::HardcodedSecret),
            "Secret hardcodeado"
        );
        assert_eq!(
            format!("{}", FindingCategory::XssVulnerability),
            "Cross-Site Scripting (XSS)"
        );
        assert_eq!(
            format!("{}", FindingCategory::CommandInjection),
            "Inyección de comandos"
        );
    }

    #[test]
    fn test_audit_result_new() {
        let result = AuditResult::new("my-proj", "/path/to/proj");
        assert_eq!(result.project_name, "my-proj");
        assert_eq!(result.project_path, "/path/to/proj");
        assert!(result.findings.is_empty());
        assert_eq!(result.summary.total, 0);
        assert_eq!(result.summary.risk_score, 0.0);
    }

    #[test]
    fn test_add_finding_counts() {
        let mut result = AuditResult::new("test", "/test");
        result.add_finding(make_finding(
            Severity::Critical,
            FindingCategory::HardcodedSecret,
        ));
        result.add_finding(make_finding(Severity::High, FindingCategory::SqlInjection));
        result.add_finding(make_finding(
            Severity::Medium,
            FindingCategory::InsecureConfig,
        ));
        result.add_finding(make_finding(Severity::Low, FindingCategory::DebugEnabled));
        result.add_finding(make_finding(
            Severity::Info,
            FindingCategory::InsecureConfig,
        ));

        assert_eq!(result.summary.total, 5);
        assert_eq!(result.summary.critical, 1);
        assert_eq!(result.summary.high, 1);
        assert_eq!(result.summary.medium, 1);
        assert_eq!(result.summary.low, 1);
        assert_eq!(result.summary.info, 1);
        assert_eq!(result.findings.len(), 5);
    }

    #[test]
    fn test_risk_score_critical() {
        let mut result = AuditResult::new("test", "/test");
        result.add_finding(make_finding(
            Severity::Critical,
            FindingCategory::HardcodedSecret,
        ));
        result.compute_risk_score();
        assert_eq!(result.summary.risk_score, 40.0);
    }

    #[test]
    fn test_risk_score_mixed() {
        let mut result = AuditResult::new("test", "/test");
        result.add_finding(make_finding(
            Severity::Critical,
            FindingCategory::HardcodedSecret,
        ));
        result.add_finding(make_finding(Severity::High, FindingCategory::SqlInjection));
        result.add_finding(make_finding(
            Severity::Medium,
            FindingCategory::InsecureConfig,
        ));
        result.compute_risk_score();
        // 40 + 20 + 5 = 65
        assert_eq!(result.summary.risk_score, 65.0);
    }

    #[test]
    fn test_risk_score_capped_at_100() {
        let mut result = AuditResult::new("test", "/test");
        for _ in 0..5 {
            result.add_finding(make_finding(
                Severity::Critical,
                FindingCategory::HardcodedSecret,
            ));
        }
        result.compute_risk_score();
        // 5 * 40 = 200, capped at 100
        assert_eq!(result.summary.risk_score, 100.0);
    }

    #[test]
    fn test_risk_score_zero_findings() {
        let mut result = AuditResult::new("test", "/test");
        result.compute_risk_score();
        assert_eq!(result.summary.risk_score, 0.0);
    }

    #[test]
    fn test_finding_serde_roundtrip() {
        let finding = make_finding(Severity::High, FindingCategory::XssVulnerability);
        let json = serde_json::to_string(&finding).unwrap();
        let loaded: SecurityFinding = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.severity, Severity::High);
        assert_eq!(loaded.category, FindingCategory::XssVulnerability);
    }

    #[test]
    fn test_audit_summary_default() {
        let summary = AuditSummary::default();
        assert_eq!(summary.total, 0);
        assert_eq!(summary.critical, 0);
        assert_eq!(summary.risk_score, 0.0);
    }
}
