use std::path::Path;

use async_trait::async_trait;

use super::{CheckStatus, DependencyDetector, DependencyStatus, DependencyType, run_cmd};

pub struct FlutterDetector;

#[async_trait]
impl DependencyDetector for FlutterDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Flutter
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("pubspec.yaml").exists()
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Flutter);

        // Check flutter
        let flutter_ver = run_cmd("flutter", &["--version"]).await;
        match flutter_ver {
            Some(ver) => {
                // "Flutter 3.19.4 • channel stable • ..." → "3.19.4"
                let ver_clean = ver
                    .lines()
                    .next()
                    .unwrap_or(&ver)
                    .strip_prefix("Flutter ")
                    .and_then(|s| s.split_whitespace().next())
                    .unwrap_or(&ver)
                    .to_string();
                status.version = Some(ver_clean.clone());
                status.details.push(format!("Flutter {}", ver_clean));
            }
            None => {
                return DependencyStatus {
                    dep_type: DependencyType::Flutter,
                    status: CheckStatus::Missing,
                    version: None,
                    details: vec!["flutter not found in PATH".into()],
                    fix_hint: Some("https://docs.flutter.dev/get-started/install".into()),
                };
            }
        }

        // Check dart
        if let Some(dart_ver) = run_cmd("dart", &["--version"]).await {
            // "Dart SDK version: 3.3.2 (stable) ..." → "3.3.2"
            let ver = dart_ver
                .strip_prefix("Dart SDK version: ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or(&dart_ver);
            status.details.push(format!("Dart {}", ver));
        }

        status
    }
}
