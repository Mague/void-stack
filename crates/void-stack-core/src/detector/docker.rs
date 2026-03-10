use std::path::Path;

use async_trait::async_trait;

use super::{run_cmd, run_cmd_any, CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct DockerDetector;

#[async_trait]
impl DependencyDetector for DockerDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Docker
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("Dockerfile").exists()
            || project_path.join("docker-compose.yml").exists()
            || project_path.join("docker-compose.yaml").exists()
            || project_path.join("compose.yml").exists()
            || project_path.join("compose.yaml").exists()
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Docker);

        // Check docker binary
        let docker_ver = run_cmd("docker", &["--version"]).await;
        match docker_ver {
            Some(ver) => {
                // "Docker version 24.0.7, build afdd53b" → "24.0.7"
                let ver_clean = ver
                    .strip_prefix("Docker version ")
                    .and_then(|s| s.split(',').next())
                    .unwrap_or(&ver)
                    .to_string();
                status.version = Some(ver_clean.clone());
                status.details.push(format!("Docker {}", ver_clean));
            }
            None => {
                return DependencyStatus {
                    dep_type: DependencyType::Docker,
                    status: CheckStatus::Missing,
                    version: None,
                    details: vec!["Docker not found in PATH".into()],
                    fix_hint: Some(crate::process_util::install_hint("docker")),
                };
            }
        }

        // Check if Docker daemon is running
        let info = run_cmd_any("docker", &["info", "--format", "{{.ServerVersion}}"]).await;
        match info {
            Some(ver) if !ver.is_empty() && !ver.contains("error") => {
                status.details.push(format!("Daemon running (server {})", ver.trim()));
            }
            _ => {
                status.status = CheckStatus::NotRunning;
                status.details.push("Docker daemon not running".into());
                status.fix_hint = Some("Start Docker Desktop or run: dockerd".into());
                return status;
            }
        }

        // Check docker compose
        let compose_ver = run_cmd("docker", &["compose", "version", "--short"]).await;
        if let Some(ver) = compose_ver {
            status.details.push(format!("Compose {}", ver));
        } else {
            // Try legacy docker-compose
            let legacy = run_cmd("docker-compose", &["--version"]).await;
            if let Some(ver) = legacy {
                status.details.push(format!("docker-compose: {}", ver));
            } else {
                status.details.push("docker compose: not available".into());
            }
        }

        status
    }
}
