//! Dependency detection system.
//!
//! Each detector checks whether a specific runtime dependency is available,
//! properly configured, and ready for use. Results include actionable fix hints.

pub mod clippy;
pub mod cuda;
pub mod docker;
pub mod env;
pub mod flutter;
pub mod flutter_analyze;
pub mod golang;
pub mod golangci_lint;
pub mod node;
pub mod ollama;
pub mod python;
pub mod react_doctor;
pub mod ruff;
pub mod rust_lang;

use std::path::Path;
use std::sync::OnceLock;
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

static USER_SHELL_PATH: OnceLock<String> = OnceLock::new();

/// Resolve the full user PATH from a login shell on macOS.
///
/// GUI apps launched from Finder/Dock inherit a minimal PATH
/// (`/usr/bin:/bin:/usr/sbin:/sbin`) that excludes Homebrew, NVM, Volta,
/// Cargo and other developer tool directories.
///
/// Two-layer approach:
/// 1. Spawn a login shell with `TERM` set to force tool initialization
///    (NVM, Volta, pyenv check for TERM before loading). Validate the
///    result is long enough (>20 chars) to detect incomplete PATH.
/// 2. If the shell fails, manually construct PATH by checking known
///    developer tool locations with `Path::exists()`.
///
/// Result is cached for the process lifetime via `OnceLock`.
fn get_user_shell_path() -> &'static str {
    USER_SHELL_PATH.get_or_init(|| {
        // Layer 1: login shell with TERM forced
        for shell in &["/bin/zsh", "/bin/bash"] {
            if let Ok(output) = std::process::Command::new(shell)
                .args(["-l", "-c", "echo $PATH"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .env("TERM", "xterm-256color")
                .output()
            {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if path.len() > 20 {
                    return path;
                }
            }
        }

        // Layer 2: manually construct PATH from known developer tool locations
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            format!("{home}/.cargo/bin"),
            format!("{home}/.volta/bin"),
            "/opt/homebrew/bin".to_string(),
            "/opt/homebrew/sbin".to_string(),
            "/usr/local/bin".to_string(),
            "/usr/local/sbin".to_string(),
            format!("{home}/.pyenv/shims"),
            format!("{home}/.rbenv/shims"),
        ];

        // Check for NVM: find the latest installed node version
        let nvm_bin = std::fs::read_dir(format!("{home}/.nvm/versions/node"))
            .ok()
            .and_then(|entries| {
                let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                versions.sort_by_key(|e| e.file_name());
                versions
                    .last()
                    .map(|v| format!("{}/bin", v.path().display()))
            });

        let current = std::env::var("PATH").unwrap_or_default();
        let mut extra: Vec<String> = Vec::new();

        if let Some(nvm) = nvm_bin.filter(|p| std::path::Path::new(p).exists()) {
            extra.push(nvm);
        }

        for candidate in &candidates {
            if std::path::Path::new(candidate.as_str()).exists() {
                extra.push(candidate.clone());
            }
        }

        if extra.is_empty() {
            current
        } else {
            extra.push(current);
            extra.join(":")
        }
    })
}

/// Run a command with a timeout and return its stdout as a string.
/// Returns None if the command fails or times out.
pub(crate) async fn run_cmd(program: &str, args: &[&str]) -> Option<String> {
    use crate::process_util::HideWindow;
    let result = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .env("PATH", get_user_shell_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .hide_window()
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
    use crate::process_util::HideWindow;
    let result = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .env("PATH", get_user_shell_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .hide_window()
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => Some(String::from_utf8_lossy(&output.stdout).trim().to_string()),
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
    fn test_all_detectors_count() {
        let detectors = all_detectors();
        assert_eq!(detectors.len(), 14);
    }

    #[test]
    fn test_dependency_type_serde() {
        let json = serde_json::to_string(&DependencyType::Python).unwrap();
        assert_eq!(json, "\"python\"");
        let json = serde_json::to_string(&DependencyType::GolangciLint).unwrap();
        assert_eq!(json, "\"golangcilint\"");
    }
}
