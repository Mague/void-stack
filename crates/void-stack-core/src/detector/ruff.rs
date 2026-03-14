use std::path::Path;

use async_trait::async_trait;

use super::{CheckStatus, DependencyDetector, DependencyStatus, DependencyType, run_cmd};

pub struct RuffDetector;

#[async_trait]
impl DependencyDetector for RuffDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Ruff
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("requirements.txt").exists()
            || project_path.join("pyproject.toml").exists()
            || project_path.join("setup.py").exists()
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        match run_cmd("ruff", &["--version"]).await {
            Some(ver) => {
                let mut status = DependencyStatus::ok(DependencyType::Ruff);
                status.version = Some(ver.trim().to_string());
                status.details.push(format!("ruff {}", ver.trim()));
                status
            }
            None => DependencyStatus {
                dep_type: DependencyType::Ruff,
                status: CheckStatus::Missing,
                version: None,
                details: vec!["ruff not found in PATH".into()],
                fix_hint: Some("pip install ruff".into()),
            },
        }
    }
}
