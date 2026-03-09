use std::path::Path;

use async_trait::async_trait;

use super::{run_cmd, CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct FlutterAnalyzeDetector;

#[async_trait]
impl DependencyDetector for FlutterAnalyzeDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::FlutterAnalyze
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("pubspec.yaml").exists()
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        // Try flutter first, fallback to dart
        if let Some(ver) = run_cmd("flutter", &["--version"]).await {
            let mut status = DependencyStatus::ok(DependencyType::FlutterAnalyze);
            let ver_clean = ver.lines().next().unwrap_or(ver.trim());
            status.version = Some(ver_clean.to_string());
            status.details.push("flutter analyze disponible".into());
            return status;
        }

        if let Some(ver) = run_cmd("dart", &["--version"]).await {
            let mut status = DependencyStatus::ok(DependencyType::FlutterAnalyze);
            status.version = Some(ver.trim().to_string());
            status.details.push("dart analyze disponible".into());
            return status;
        }

        DependencyStatus {
            dep_type: DependencyType::FlutterAnalyze,
            status: CheckStatus::Missing,
            version: None,
            details: vec!["flutter/dart not found in PATH".into()],
            fix_hint: Some("Instalar Flutter SDK desde https://flutter.dev".into()),
        }
    }
}
