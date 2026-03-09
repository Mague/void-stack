use std::path::Path;

use async_trait::async_trait;

use super::{run_cmd, CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct ReactDoctorDetector;

#[async_trait]
impl DependencyDetector for ReactDoctorDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::ReactDoctor
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        let pkg = project_path.join("package.json");
        if !pkg.exists() { return false; }
        std::fs::read_to_string(&pkg)
            .map(|c| c.contains("\"react\""))
            .unwrap_or(false)
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        // react-doctor runs via npx, so check if npx is available
        match run_cmd("npx", &["--version"]).await {
            Some(_) => {
                let mut status = DependencyStatus::ok(DependencyType::ReactDoctor);
                status.details.push("Disponible via npx — no requiere instalación".into());
                status
            }
            None => DependencyStatus {
                dep_type: DependencyType::ReactDoctor,
                status: CheckStatus::Missing,
                version: None,
                details: vec!["npx not found — Node.js required".into()],
                fix_hint: Some("Instalar Node.js para usar npx react-doctor".into()),
            },
        }
    }
}
