//! Dependency detection system.
//!
//! Each detector checks whether a specific runtime dependency is available,
//! properly configured, and ready for use. Results include actionable fix hints.

pub mod cuda;
pub mod docker;
pub mod env;
pub mod flutter;
pub mod golang;
pub mod node;
pub mod ollama;
pub mod python;
pub mod rust_lang;
pub mod ruff;
pub mod clippy;
pub mod golangci_lint;
pub mod flutter_analyze;
pub mod react_doctor;

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
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

/// Default timeout for running external commands.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Trait for dependency detectors.
#[async_trait]
pub trait DependencyDetector: Send + Sync {
    /// Which dependency this detector checks.
    fn dep_type(&self) -> DependencyType;

    /// Check if this dependency is relevant for the given project directory.
    /// Returns false if this detector should be skipped.
    fn is_relevant(&self, project_path: &Path) -> bool;

    /// Run the check. Must complete within the timeout.
    async fn check(&self, project_path: &Path) -> DependencyStatus;
}

/// Run a command with a timeout and return its stdout as a string.
/// Returns None if the command fails or times out.
pub(crate) async fn run_cmd(program: &str, args: &[&str]) -> Option<String> {
    let result = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        _ => None,
    }
}

/// Run a command and return stdout even if exit code is non-zero.
pub(crate) async fn run_cmd_any(program: &str, args: &[&str]) -> Option<String> {
    let result = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        _ => None,
    }
}

/// Build all available detectors.
pub fn all_detectors() -> Vec<Box<dyn DependencyDetector>> {
    vec![
        Box::new(python::PythonDetector),
        Box::new(node::NodeDetector),
        Box::new(cuda::CudaDetector),
        Box::new(ollama::OllamaDetector),
        Box::new(docker::DockerDetector),
        Box::new(rust_lang::RustDetector),
        Box::new(golang::GoDetector),
        Box::new(flutter::FlutterDetector),
        Box::new(env::EnvDetector),
        Box::new(ruff::RuffDetector),
        Box::new(clippy::ClippyDetector),
        Box::new(golangci_lint::GolangciLintDetector),
        Box::new(flutter_analyze::FlutterAnalyzeDetector),
        Box::new(react_doctor::ReactDoctorDetector),
    ]
}

/// Check all relevant dependencies for a project directory.
/// Runs detectors in parallel with a global timeout of 10 seconds.
pub async fn check_project(project_path: &Path) -> Vec<DependencyStatus> {
    let detectors = all_detectors();
    let relevant: Vec<_> = detectors
        .into_iter()
        .filter(|d| d.is_relevant(project_path))
        .collect();

    let mut handles = Vec::new();
    for detector in relevant {
        let path = project_path.to_path_buf();
        handles.push(tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(10), detector.check(&path))
                .await
                .unwrap_or_else(|_| DependencyStatus {
                    dep_type: detector.dep_type(),
                    status: CheckStatus::Unknown,
                    version: None,
                    details: vec!["Timeout checking dependency".into()],
                    fix_hint: None,
                })
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(status) = handle.await {
            results.push(status);
        }
    }
    results
}
