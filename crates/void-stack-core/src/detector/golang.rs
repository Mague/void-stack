use std::path::Path;

use async_trait::async_trait;

use super::{run_cmd, CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct GoDetector;

#[async_trait]
impl DependencyDetector for GoDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Go
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("go.mod").exists()
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Go);

        // Check go
        let go_ver = run_cmd("go", &["version"]).await;
        match go_ver {
            Some(ver) => {
                // "go version go1.22.1 windows/amd64" → "1.22.1"
                let ver_clean = ver
                    .split_whitespace()
                    .find(|s| s.starts_with("go1") || s.starts_with("go0"))
                    .and_then(|s| s.strip_prefix("go"))
                    .unwrap_or(&ver)
                    .to_string();
                status.version = Some(ver_clean.clone());
                status.details.push(format!("Go {}", ver_clean));
            }
            None => {
                return DependencyStatus {
                    dep_type: DependencyType::Go,
                    status: CheckStatus::Missing,
                    version: None,
                    details: vec!["go not found in PATH".into()],
                    fix_hint: Some("winget install GoLang.Go".into()),
                };
            }
        }

        status
    }
}
