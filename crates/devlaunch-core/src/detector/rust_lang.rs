use std::path::Path;

use async_trait::async_trait;

use super::{run_cmd, CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct RustDetector;

#[async_trait]
impl DependencyDetector for RustDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Rust
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("Cargo.toml").exists()
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Rust);

        // Check rustc
        let rustc_ver = run_cmd("rustc", &["--version"]).await;
        match rustc_ver {
            Some(ver) => {
                // "rustc 1.78.0 (9b00956e5 2024-04-29)" → "1.78.0"
                let ver_clean = ver
                    .strip_prefix("rustc ")
                    .and_then(|s| s.split_whitespace().next())
                    .unwrap_or(&ver)
                    .to_string();
                status.version = Some(ver_clean.clone());
                status.details.push(format!("rustc {}", ver_clean));
            }
            None => {
                return DependencyStatus {
                    dep_type: DependencyType::Rust,
                    status: CheckStatus::Missing,
                    version: None,
                    details: vec!["rustc not found in PATH".into()],
                    fix_hint: Some("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh".into()),
                };
            }
        }

        // Check cargo
        if let Some(cargo_ver) = run_cmd("cargo", &["--version"]).await {
            let ver = cargo_ver
                .strip_prefix("cargo ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or(&cargo_ver);
            status.details.push(format!("cargo {}", ver));
        }

        status
    }
}
