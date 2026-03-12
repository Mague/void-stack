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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl AuditResult {
    pub fn new(project_name: &str, project_path: &str) -> Self {
        Self {
            project_name: project_name.into(),
            project_path: project_path.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            findings: Vec::new(),
            summary: AuditSummary::default(),
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
