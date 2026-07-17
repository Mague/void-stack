//! Dependency detection system.
//!
//! Each detector checks whether a specific runtime dependency is available,
//! properly configured, and ready for use. Results include actionable fix hints.

pub mod clippy;
pub mod cuda;
pub mod docker;
pub mod env;
mod exec;
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
mod types;

pub(crate) use exec::{run_cmd, run_cmd_any};
pub use types::{CheckStatus, DependencyStatus, DependencyType};

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;

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
    fn test_all_detectors_count() {
        let detectors = all_detectors();
        assert_eq!(detectors.len(), 14);
    }

    // ── is_relevant tests ────────────────────────────────────

    #[test]
    fn test_python_relevant_with_requirements() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        assert!(python::PythonDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_python_relevant_with_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[tool.poetry]\n").unwrap();
        assert!(python::PythonDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_python_not_relevant_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!python::PythonDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_node_relevant_with_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert!(node::NodeDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_node_not_relevant_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!node::NodeDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_rust_relevant_with_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        assert!(rust_lang::RustDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_rust_not_relevant_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!rust_lang::RustDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_go_relevant_with_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example\n").unwrap();
        assert!(golang::GoDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_go_not_relevant_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!golang::GoDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_flutter_relevant_with_pubspec() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app\n").unwrap();
        assert!(flutter::FlutterDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_flutter_not_relevant_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!flutter::FlutterDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_docker_relevant_with_compose() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("docker-compose.yml"), "version: '3'\n").unwrap();
        assert!(docker::DockerDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_docker_relevant_with_dockerfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Dockerfile"), "FROM node\n").unwrap();
        assert!(docker::DockerDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_docker_not_relevant_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!docker::DockerDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_env_relevant_with_example() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env.example"), "SECRET=xxx\n").unwrap();
        assert!(env::EnvDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_env_relevant_with_sample() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env.sample"), "KEY=val\n").unwrap();
        assert!(env::EnvDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_env_not_relevant_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!env::EnvDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_cuda_relevant_with_torch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "torch\ncuda\n").unwrap();
        assert!(cuda::CudaDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_cuda_not_relevant_without_gpu() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\nrequests\n").unwrap();
        assert!(!cuda::CudaDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_cuda_not_relevant_no_requirements() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!cuda::CudaDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_clippy_relevant_with_cargo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        assert!(clippy::ClippyDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_ruff_relevant_with_python() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "ruff\n").unwrap();
        assert!(ruff::RuffDetector.is_relevant(dir.path()));
    }

    #[test]
    fn test_golangci_relevant_with_go() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x\n").unwrap();
        assert!(golangci_lint::GolangciLintDetector.is_relevant(dir.path()));
    }
}
