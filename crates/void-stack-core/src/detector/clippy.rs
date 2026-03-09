use std::path::Path;

use async_trait::async_trait;

use super::{run_cmd, CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct ClippyDetector;

#[async_trait]
impl DependencyDetector for ClippyDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Clippy
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("Cargo.toml").exists()
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        match run_cmd("cargo", &["clippy", "--version"]).await {
            Some(ver) => {
                let mut status = DependencyStatus::ok(DependencyType::Clippy);
                let ver_clean = ver.split_whitespace().nth(1).unwrap_or(ver.trim());
                status.version = Some(ver_clean.to_string());
                status.details.push(format!("clippy {}", ver_clean));
                status
            }
            None => DependencyStatus {
                dep_type: DependencyType::Clippy,
                status: CheckStatus::Missing,
                version: None,
                details: vec!["cargo clippy not found".into()],
                fix_hint: Some("rustup component add clippy".into()),
            },
        }
    }
}
