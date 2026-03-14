use std::path::Path;

use async_trait::async_trait;

use super::{CheckStatus, DependencyDetector, DependencyStatus, DependencyType, run_cmd};

pub struct GolangciLintDetector;

#[async_trait]
impl DependencyDetector for GolangciLintDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::GolangciLint
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("go.mod").exists()
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        match run_cmd("golangci-lint", &["--version"]).await {
            Some(ver) => {
                let mut status = DependencyStatus::ok(DependencyType::GolangciLint);
                // "golangci-lint has version 1.55.2 ..." → extract version
                let ver_clean = ver
                    .split_whitespace()
                    .find(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()))
                    .unwrap_or(ver.trim());
                status.version = Some(ver_clean.to_string());
                status.details.push(format!("golangci-lint {}", ver_clean));
                status
            }
            None => DependencyStatus {
                dep_type: DependencyType::GolangciLint,
                status: CheckStatus::Missing,
                version: None,
                details: vec!["golangci-lint not found in PATH".into()],
                fix_hint: Some(
                    "go install github.com/golangci/golangci-lint/cmd/golangci-lint@latest".into(),
                ),
            },
        }
    }
}
