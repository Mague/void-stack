use std::path::Path;

use async_trait::async_trait;

use super::{CheckStatus, DependencyDetector, DependencyStatus, DependencyType, run_cmd};

pub struct NodeDetector;

#[async_trait]
impl DependencyDetector for NodeDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Node
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("package.json").exists()
    }

    async fn check(&self, project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Node);

        // Check node
        let node_ver = run_cmd("node", &["--version"]).await;
        match node_ver {
            Some(ver) => {
                let ver_clean = ver.strip_prefix('v').unwrap_or(&ver).to_string();
                status.version = Some(ver_clean.clone());
                status.details.push(format!("Node {}", ver_clean));
            }
            None => {
                return DependencyStatus {
                    dep_type: DependencyType::Node,
                    status: CheckStatus::Missing,
                    version: None,
                    details: vec!["Node.js not found in PATH".into()],
                    fix_hint: Some(crate::process_util::install_hint("node")),
                };
            }
        }

        // Check npm
        if let Some(npm_ver) = run_cmd("npm", &["--version"]).await {
            status.details.push(format!("npm {}", npm_ver));
        }

        // Check node_modules
        let node_modules = project_path.join("node_modules");
        if !node_modules.exists() {
            status.status = CheckStatus::NeedsSetup;
            status.details.push("node_modules/ not found".into());
            status.fix_hint = Some("npm install".into());
            return status;
        }

        // Quick staleness check: is package.json newer than node_modules?
        let pkg_modified = project_path
            .join("package.json")
            .metadata()
            .and_then(|m| m.modified())
            .ok();
        let nm_modified = node_modules.metadata().and_then(|m| m.modified()).ok();

        if let (Some(pkg_time), Some(nm_time)) = (pkg_modified, nm_modified)
            && pkg_time > nm_time
        {
            status.status = CheckStatus::NeedsSetup;
            status
                .details
                .push("node_modules may be outdated (package.json is newer)".into());
            status.fix_hint = Some("npm install".into());
            return status;
        }

        status.details.push("node_modules/ present".into());
        status
    }
}
