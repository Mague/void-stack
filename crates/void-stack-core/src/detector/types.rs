//! Core types for the dependency detection system: dependency kinds,
//! check statuses and the full per-dependency result.

use serde::Serialize;

/// Types of dependencies that VoidStack can detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyType {
    Python,
    Node,
    Cuda,
    Ollama,
    Docker,
    Rust,
    Go,
    Flutter,
    Env,
    Ruff,
    Clippy,
    GolangciLint,
    FlutterAnalyze,
    ReactDoctor,
}

impl std::fmt::Display for DependencyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DependencyType::Python => write!(f, "Python"),
            DependencyType::Node => write!(f, "Node.js"),
            DependencyType::Cuda => write!(f, "CUDA"),
            DependencyType::Ollama => write!(f, "Ollama"),
            DependencyType::Docker => write!(f, "Docker"),
            DependencyType::Rust => write!(f, "Rust"),
            DependencyType::Go => write!(f, "Go"),
            DependencyType::Flutter => write!(f, "Flutter"),
            DependencyType::Env => write!(f, ".env"),
            DependencyType::Ruff => write!(f, "Ruff"),
            DependencyType::Clippy => write!(f, "Clippy"),
            DependencyType::GolangciLint => write!(f, "golangci-lint"),
            DependencyType::FlutterAnalyze => write!(f, "Flutter Analyze"),
            DependencyType::ReactDoctor => write!(f, "react-doctor"),
        }
    }
}

/// Result of checking a single dependency.
#[derive(Debug, Clone, Serialize)]
pub enum CheckStatus {
    /// Dependency is available and ready.
    Ok,
    /// Dependency is not installed or not found.
    Missing,
    /// Dependency is installed but not running (e.g., Ollama, Docker daemon).
    NotRunning,
    /// Dependency exists but needs setup (e.g., missing node_modules, .env).
    NeedsSetup,
    /// Could not determine status (timeout, unexpected error).
    Unknown,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckStatus::Ok => write!(f, "OK"),
            CheckStatus::Missing => write!(f, "MISSING"),
            CheckStatus::NotRunning => write!(f, "NOT RUNNING"),
            CheckStatus::NeedsSetup => write!(f, "NEEDS SETUP"),
            CheckStatus::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

/// Full result of a dependency check.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyStatus {
    pub dep_type: DependencyType,
    pub status: CheckStatus,
    /// Version found (e.g., "3.11.5", "20.10.7").
    pub version: Option<String>,
    /// Human-readable details about what was found/missing.
    pub details: Vec<String>,
    /// Actionable command to fix the issue (copy-pasteable).
    pub fix_hint: Option<String>,
}

impl DependencyStatus {
    pub fn ok(dep_type: DependencyType) -> Self {
        Self {
            dep_type,
            status: CheckStatus::Ok,
            version: None,
            details: vec![],
            fix_hint: None,
        }
    }

    pub fn missing(dep_type: DependencyType, fix_hint: &str) -> Self {
        Self {
            dep_type,
            status: CheckStatus::Missing,
            version: None,
            details: vec![],
            fix_hint: Some(fix_hint.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_type_display() {
        assert_eq!(format!("{}", DependencyType::Python), "Python");
        assert_eq!(format!("{}", DependencyType::Node), "Node.js");
        assert_eq!(format!("{}", DependencyType::Cuda), "CUDA");
        assert_eq!(format!("{}", DependencyType::Ollama), "Ollama");
        assert_eq!(format!("{}", DependencyType::Docker), "Docker");
        assert_eq!(format!("{}", DependencyType::Rust), "Rust");
        assert_eq!(format!("{}", DependencyType::Go), "Go");
        assert_eq!(format!("{}", DependencyType::Flutter), "Flutter");
        assert_eq!(format!("{}", DependencyType::Env), ".env");
        assert_eq!(format!("{}", DependencyType::Ruff), "Ruff");
        assert_eq!(format!("{}", DependencyType::Clippy), "Clippy");
        assert_eq!(format!("{}", DependencyType::GolangciLint), "golangci-lint");
        assert_eq!(
            format!("{}", DependencyType::FlutterAnalyze),
            "Flutter Analyze"
        );
        assert_eq!(format!("{}", DependencyType::ReactDoctor), "react-doctor");
    }

    #[test]
    fn test_check_status_display() {
        assert_eq!(format!("{}", CheckStatus::Ok), "OK");
        assert_eq!(format!("{}", CheckStatus::Missing), "MISSING");
        assert_eq!(format!("{}", CheckStatus::NotRunning), "NOT RUNNING");
        assert_eq!(format!("{}", CheckStatus::NeedsSetup), "NEEDS SETUP");
        assert_eq!(format!("{}", CheckStatus::Unknown), "UNKNOWN");
    }

    #[test]
    fn test_dependency_status_ok() {
        let status = DependencyStatus::ok(DependencyType::Python);
        assert!(matches!(status.status, CheckStatus::Ok));
        assert!(status.version.is_none());
        assert!(status.fix_hint.is_none());
    }

    #[test]
    fn test_dependency_status_missing() {
        let status = DependencyStatus::missing(DependencyType::Node, "npm install");
        assert!(matches!(status.status, CheckStatus::Missing));
        assert_eq!(status.fix_hint.as_deref(), Some("npm install"));
    }

    #[test]
    fn test_dependency_type_serde() {
        let json = serde_json::to_string(&DependencyType::Python).unwrap();
        assert_eq!(json, "\"python\"");
        let json = serde_json::to_string(&DependencyType::GolangciLint).unwrap();
        assert_eq!(json, "\"golangcilint\"");
    }
}
