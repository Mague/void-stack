//! Helm chart parser — extract chart metadata and dependencies.

use std::path::Path;

use super::{HelmChart, HelmDependency};

/// Find and parse Helm charts in a project directory.
pub fn parse_helm(project_path: &Path) -> Option<HelmChart> {
    // Search for Chart.yaml in common locations
    let candidates = [
        project_path.join("Chart.yaml"),
        project_path.join("chart").join("Chart.yaml"),
        project_path.join("helm").join("Chart.yaml"),
        project_path.join("charts").join("Chart.yaml"),
        project_path.join("deploy").join("Chart.yaml"),
        project_path.join("deploy").join("helm").join("Chart.yaml"),
    ];

    // Also scan for Chart.yaml one level deep
    let mut chart_paths: Vec<std::path::PathBuf> = candidates.iter()
        .filter(|p| p.exists())
        .cloned()
        .collect();

    // Scan subdirectories one level deep for Chart.yaml
    if chart_paths.is_empty() {
        if let Ok(entries) = std::fs::read_dir(project_path) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let candidate = entry.path().join("Chart.yaml");
                    if candidate.exists() {
                        chart_paths.push(candidate);
                    }
                }
            }
        }
    }

    // Take the first found Chart.yaml
    let chart_path = chart_paths.into_iter().next()?;
    parse_chart_yaml(&chart_path)
}

fn parse_chart_yaml(path: &Path) -> Option<HelmChart> {
    let content = std::fs::read_to_string(path).ok()?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;

    let name = doc.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let version = doc.get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    let mut dependencies = Vec::new();

    if let Some(deps) = doc.get("dependencies").and_then(|v| v.as_sequence()) {
        for dep in deps {
            let dep_name = dep.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let dep_version = dep.get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            let dep_repo = dep.get("repository")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if !dep_name.is_empty() {
                dependencies.push(HelmDependency {
                    name: dep_name,
                    version: dep_version,
                    repository: dep_repo,
                });
            }
        }
    }

    Some(HelmChart {
        name,
        version,
        dependencies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_helm_chart() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Chart.yaml"), r#"
apiVersion: v2
name: my-application
description: A Helm chart for my app
version: 1.2.3
appVersion: "2.0.0"
dependencies:
  - name: postgresql
    version: "12.1.0"
    repository: "https://charts.bitnami.com/bitnami"
  - name: redis
    version: "17.0.0"
    repository: "https://charts.bitnami.com/bitnami"
  - name: rabbitmq
    version: "11.0.0"
    repository: "https://charts.bitnami.com/bitnami"
"#).unwrap();

        let chart = parse_helm(dir.path()).unwrap();
        assert_eq!(chart.name, "my-application");
        assert_eq!(chart.version, "1.2.3");
        assert_eq!(chart.dependencies.len(), 3);
        assert_eq!(chart.dependencies[0].name, "postgresql");
        assert_eq!(chart.dependencies[0].version, "12.1.0");
        assert!(chart.dependencies[0].repository.contains("bitnami"));
        assert_eq!(chart.dependencies[1].name, "redis");
        assert_eq!(chart.dependencies[2].name, "rabbitmq");
    }

    #[test]
    fn test_parse_helm_chart_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let helm_dir = dir.path().join("helm");
        std::fs::create_dir(&helm_dir).unwrap();

        std::fs::write(helm_dir.join("Chart.yaml"), r#"
apiVersion: v2
name: api-chart
version: 0.1.0
"#).unwrap();

        let chart = parse_helm(dir.path()).unwrap();
        assert_eq!(chart.name, "api-chart");
        assert_eq!(chart.version, "0.1.0");
        assert!(chart.dependencies.is_empty());
    }

    #[test]
    fn test_parse_helm_no_chart() {
        let dir = tempfile::tempdir().unwrap();
        assert!(parse_helm(dir.path()).is_none());
    }

    #[test]
    fn test_parse_helm_chart_no_deps() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Chart.yaml"), r#"
apiVersion: v2
name: simple-chart
version: 1.0.0
"#).unwrap();

        let chart = parse_helm(dir.path()).unwrap();
        assert_eq!(chart.name, "simple-chart");
        assert!(chart.dependencies.is_empty());
    }
}
