use serde::{Deserialize, Serialize};
use std::fmt;

/// Severity level for security findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

// ── Contextual enrichment types ─────────────────────────────

/// Confidence level for the severity adjustment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Confidence {
    Certain,
    Probable,
    #[default]
    Heuristic,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Certain => write!(f, "C"),
            Self::Probable => write!(f, "P"),
            Self::Heuristic => write!(f, "H"),
        }
    }
}

/// Role of the module where the finding lives — drives severity adjustments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ModuleRole {
    #[default]
    Core,
    /// `src/audit/` but NOT vuln_patterns/ — real audit logic.
    Audit,
    /// `src/audit/vuln_patterns/` — detection patterns and fixtures, safe to silence.
    AuditPattern,
    CLI,
    Test,
    Generated,
    I18n,
    Example,
    Migration,
}

/// Syntactic context around a finding.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindingContext {
    pub in_test_file: bool,
    pub in_const_context: bool,
    pub in_hot_path: bool,
    pub callers_count: usize,
    pub module_role: ModuleRole,
    pub language: String,
    #[serde(default)]
    pub surrounding_lines: String,
}

// ── Finding ─────────────────────────────────────────────────

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
    /// Severity after contextual adjustment. Defaults to `severity` until
    /// the enrichment pipeline runs.
    #[serde(default)]
    pub adjusted_severity: Option<Severity>,
    /// How confident the adjustment is.
    #[serde(default)]
    pub confidence: Confidence,
    /// Why the severity was changed (empty when it wasn't).
    #[serde(default)]
    pub adjustment_reason: Option<String>,
    /// Contextual metadata populated by the enrichment pipeline.
    #[serde(default)]
    pub context: FindingContext,
}

impl SecurityFinding {
    /// Shorthand constructor — fills enrichment fields with safe defaults.
    /// The enrichment pipeline overwrites them later.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        severity: Severity,
        category: FindingCategory,
        title: String,
        description: String,
        file_path: Option<String>,
        line_number: Option<u32>,
        remediation: String,
    ) -> Self {
        Self {
            id,
            severity,
            category,
            title,
            description,
            file_path,
            line_number,
            remediation,
            adjusted_severity: None,
            confidence: Confidence::Heuristic,
            adjustment_reason: None,
            context: FindingContext::default(),
        }
    }
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
        // Use adjusted_severity if available, else the base severity.
        let sev = finding.adjusted_severity.unwrap_or(finding.severity);
        match sev {
            Severity::Critical => self.summary.critical += 1,
            Severity::High => self.summary.high += 1,
            Severity::Medium => self.summary.medium += 1,
            Severity::Low => self.summary.low += 1,
            Severity::Info => self.summary.info += 1,
        }
        self.summary.total += 1;
        self.findings.push(finding);
    }

    /// Compute risk score weighted by adjusted_severity + confidence.
    pub fn compute_risk_score(&mut self) {
        let mut score = 0u32;
        for f in &self.findings {
            let sev = f.adjusted_severity.unwrap_or(f.severity);
            let weight = match (sev, f.confidence) {
                (Severity::Critical, Confidence::Certain) => 20,
                (Severity::Critical, Confidence::Probable) => 15,
                (Severity::Critical, Confidence::Heuristic) => 10,
                (Severity::High, Confidence::Certain) => 10,
                (Severity::High, Confidence::Probable) => 7,
                (Severity::High, Confidence::Heuristic) => 4,
                (Severity::Medium, Confidence::Certain) => 3,
                (Severity::Medium, Confidence::Probable) => 2,
                (Severity::Medium, Confidence::Heuristic) => 1,
                (Severity::Low, _) | (Severity::Info, _) => 0,
            };
            score += weight;
        }
        self.summary.risk_score = score.min(100) as f32;
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
            adjusted_severity: None,
            confidence: Confidence::Heuristic,
            adjustment_reason: None,
            context: FindingContext::default(),
        }
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Critical), "critical");
        assert_eq!(format!("{}", Severity::Info), "info");
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical < Severity::High);
        assert!(Severity::Medium < Severity::Low);
    }

    #[test]
    fn test_add_finding_counts_adjusted() {
        let mut result = AuditResult::new("test", "/test");
        let mut f = make_finding(Severity::Medium, FindingCategory::UnsafeErrorHandling);
        f.adjusted_severity = Some(Severity::Info);
        result.add_finding(f);
        // The adjusted Info should be counted, not the base Medium.
        assert_eq!(result.summary.info, 1);
        assert_eq!(result.summary.medium, 0);
    }

    #[test]
    fn test_risk_score_contextual() {
        let mut result = AuditResult::new("test", "/test");
        // 1 Critical Certain = 20 points
        let mut f = make_finding(Severity::Critical, FindingCategory::HardcodedSecret);
        f.adjusted_severity = Some(Severity::Critical);
        f.confidence = Confidence::Certain;
        result.add_finding(f);
        // 1 Medium Heuristic = 1 point
        result.add_finding(make_finding(
            Severity::Medium,
            FindingCategory::UnsafeErrorHandling,
        ));
        result.compute_risk_score();
        assert_eq!(result.summary.risk_score, 21.0);
    }

    #[test]
    fn test_risk_score_info_zero_weight() {
        let mut result = AuditResult::new("test", "/test");
        let mut f = make_finding(Severity::Medium, FindingCategory::UnsafeErrorHandling);
        f.adjusted_severity = Some(Severity::Info);
        f.confidence = Confidence::Certain;
        result.add_finding(f);
        result.compute_risk_score();
        assert_eq!(result.summary.risk_score, 0.0);
    }

    #[test]
    fn test_risk_score_capped() {
        let mut result = AuditResult::new("test", "/test");
        for _ in 0..10 {
            let mut f = make_finding(Severity::Critical, FindingCategory::HardcodedSecret);
            f.adjusted_severity = Some(Severity::Critical);
            f.confidence = Confidence::Certain;
            result.add_finding(f);
        }
        result.compute_risk_score();
        assert_eq!(result.summary.risk_score, 100.0);
    }

    #[test]
    fn test_finding_serde_backward_compat() {
        // Old JSON without new fields should deserialize with defaults.
        let json = r#"{"id":"x","severity":"Medium","category":"InsecureConfig","title":"t","description":"d","file_path":"a.py","line_number":1,"remediation":"r"}"#;
        let f: SecurityFinding = serde_json::from_str(json).unwrap();
        assert_eq!(f.adjusted_severity, None);
        assert_eq!(f.confidence, Confidence::Heuristic);
        assert!(f.adjustment_reason.is_none());
    }
}
