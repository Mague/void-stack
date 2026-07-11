//! `void bootstrap`: provision a new machine from a portable registry.
//!
//! `export` writes the global registry to a TOML file with paths relative
//! to a declared workspace root, so the same file works on machines with
//! different usernames or drive layouts. `import` remaps that root,
//! validates which paths exist on the target machine (the doctor's
//! missing-path check), registers only the valid ones and reports the
//! rest. Secrets never travel: service env_vars and docker extra_args are
//! deliberately not exported — only names, relative paths, commands,
//! targets and flags.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::global_config::GlobalConfig;
use crate::model::{DockerConfig, Project, ProjectType, Service, Target};
use crate::runner::local::strip_win_prefix;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableRegistry {
    /// The root the relative paths hang from on the SOURCE machine
    /// (informational; import declares its own root).
    pub workspace_root: String,
    pub exported_at: String,
    #[serde(default)]
    pub projects: Vec<PortableProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableProject {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Path relative to the workspace root; absolute when the project
    /// lived outside it (then `absolute = true`).
    pub path: String,
    #[serde(default)]
    pub absolute: bool,
    #[serde(default)]
    pub project_type: Option<ProjectType>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub services: Vec<PortableService>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableService {
    pub name: String,
    pub command: String,
    pub target: Target,
    /// Relative to the project path when possible.
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub docker_ports: Vec<String>,
    #[serde(default)]
    pub docker_volumes: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub imported: Vec<String>,
    /// (name, resolved path) pairs whose path doesn't exist here.
    pub missing: Vec<(String, String)>,
    /// Names already registered on this machine (left untouched).
    pub already_registered: Vec<String>,
}

/// Export the registry relative to `workspace_root`. env_vars and docker
/// extra_args are dropped on purpose (secret carriers).
pub fn export_registry(config: &GlobalConfig, workspace_root: &Path) -> PortableRegistry {
    let projects = config
        .projects
        .iter()
        .map(|p| {
            let clean = strip_win_prefix(&p.path);
            let abs = PathBuf::from(&clean);
            let (path, absolute) = match abs.strip_prefix(workspace_root) {
                Ok(rel) => (rel.to_string_lossy().replace('\\', "/"), false),
                Err(_) => (clean.clone(), true),
            };
            let services = p
                .services
                .iter()
                .map(|s| PortableService {
                    name: s.name.clone(),
                    command: s.command.clone(),
                    target: s.target,
                    // Store service dirs relative to the PROJECT so they
                    // remap for free.
                    working_dir: s.working_dir.as_ref().map(|wd| {
                        let wd_clean = strip_win_prefix(wd);
                        PathBuf::from(&wd_clean)
                            .strip_prefix(&abs)
                            .map(|r| r.to_string_lossy().replace('\\', "/"))
                            .unwrap_or(wd_clean)
                    }),
                    enabled: s.enabled,
                    depends_on: s.depends_on.clone(),
                    docker_ports: s
                        .docker
                        .as_ref()
                        .map(|d| d.ports.clone())
                        .unwrap_or_default(),
                    docker_volumes: s
                        .docker
                        .as_ref()
                        .map(|d| d.volumes.clone())
                        .unwrap_or_default(),
                })
                .collect();
            PortableProject {
                name: p.name.clone(),
                description: p.description.clone(),
                path,
                absolute,
                project_type: p.project_type,
                tags: p.tags.clone(),
                services,
            }
        })
        .collect();

    PortableRegistry {
        workspace_root: workspace_root.to_string_lossy().to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        projects,
    }
}

pub fn registry_to_toml(registry: &PortableRegistry) -> Result<String, String> {
    toml::to_string_pretty(registry).map_err(|e| e.to_string())
}

pub fn registry_from_toml(content: &str) -> Result<PortableRegistry, String> {
    toml::from_str(content).map_err(|e| e.to_string())
}

/// Import into `config` remapping the workspace root. Only paths that
/// exist on this machine register; the rest are reported, never guessed.
pub fn import_registry(
    config: &mut GlobalConfig,
    portable: &PortableRegistry,
    new_root: &Path,
) -> ImportReport {
    let mut report = ImportReport {
        imported: Vec::new(),
        missing: Vec::new(),
        already_registered: Vec::new(),
    };

    for p in &portable.projects {
        if crate::global_config::find_project(config, &p.name).is_some() {
            report.already_registered.push(p.name.clone());
            continue;
        }
        let resolved: PathBuf = if p.absolute {
            PathBuf::from(&p.path)
        } else {
            new_root.join(&p.path)
        };
        // The doctor's missing-path validation, applied up front.
        if !resolved.exists() {
            report
                .missing
                .push((p.name.clone(), resolved.to_string_lossy().to_string()));
            continue;
        }

        let services: Vec<Service> = p
            .services
            .iter()
            .map(|s| Service {
                name: s.name.clone(),
                command: s.command.clone(),
                target: s.target,
                working_dir: s.working_dir.as_ref().map(|wd| {
                    let candidate = PathBuf::from(wd);
                    if candidate.is_absolute() {
                        wd.clone()
                    } else {
                        resolved.join(candidate).to_string_lossy().to_string()
                    }
                }),
                enabled: s.enabled,
                env_vars: vec![],
                depends_on: s.depends_on.clone(),
                docker: if s.docker_ports.is_empty() && s.docker_volumes.is_empty() {
                    None
                } else {
                    Some(DockerConfig {
                        ports: s.docker_ports.clone(),
                        volumes: s.docker_volumes.clone(),
                        extra_args: vec![],
                    })
                },
            })
            .collect();

        config.projects.push(Project {
            name: p.name.clone(),
            description: p.description.clone(),
            path: resolved.to_string_lossy().to_string(),
            project_type: p.project_type,
            tags: p.tags.clone(),
            services,
            hooks: None,
        });
        report.imported.push(p.name.clone());
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str, path: &Path, env: Vec<(String, String)>) -> Project {
        Project {
            name: name.to_string(),
            path: path.to_string_lossy().to_string(),
            description: "demo".into(),
            project_type: None,
            tags: vec!["t1".into()],
            services: vec![Service {
                name: "api".into(),
                command: "cargo run".into(),
                target: Target::Windows,
                working_dir: Some(path.join("backend").to_string_lossy().to_string()),
                enabled: true,
                env_vars: env,
                depends_on: vec![],
                docker: None,
            }],
            hooks: None,
        }
    }

    #[test]
    fn test_export_relativizes_and_drops_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ws");
        let inside = root.join("app");
        let outside = tmp.path().join("elsewhere/tool");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let config = GlobalConfig {
            projects: vec![
                project(
                    "app",
                    &inside,
                    vec![("API_TOKEN".into(), "sk-secret".into())],
                ),
                project("tool", &outside, vec![]),
            ],
            ..Default::default()
        };
        let portable = export_registry(&config, &root);

        assert_eq!(portable.projects[0].path, "app");
        assert!(!portable.projects[0].absolute);
        assert_eq!(
            portable.projects[0].services[0].working_dir.as_deref(),
            Some("backend")
        );
        assert!(portable.projects[1].absolute);

        // No secrets anywhere in the serialized file.
        let toml_str = registry_to_toml(&portable).unwrap();
        assert!(!toml_str.contains("sk-secret"));
        assert!(!toml_str.contains("API_TOKEN"));
        assert!(!toml_str.contains("env_vars"));

        // And it roundtrips.
        let back = registry_from_toml(&toml_str).unwrap();
        assert_eq!(back.projects.len(), 2);
        assert_eq!(back.projects[0].name, "app");
    }

    #[test]
    fn test_import_remaps_root_and_reports_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let old_root = tmp.path().join("old-ws");
        std::fs::create_dir_all(old_root.join("app")).unwrap();
        let config = GlobalConfig {
            projects: vec![
                project("app", &old_root.join("app"), vec![]),
                project("gone", &old_root.join("gone"), vec![]),
            ],
            ..Default::default()
        };
        // "gone" is registered but its dir never existed; export keeps it
        // (export is dumb on purpose — import validates).
        let portable = export_registry(&config, &old_root);

        // New machine: only "app" exists under the new root.
        let new_root = tmp.path().join("new-ws");
        std::fs::create_dir_all(new_root.join("app").join("backend")).unwrap();

        let mut target = GlobalConfig::default();
        let report = import_registry(&mut target, &portable, &new_root);

        assert_eq!(report.imported, vec!["app"]);
        assert_eq!(report.missing.len(), 1);
        assert_eq!(report.missing[0].0, "gone");
        assert_eq!(target.projects.len(), 1);
        let imported = &target.projects[0];
        assert_eq!(
            imported.path,
            new_root.join("app").to_string_lossy().to_string()
        );
        // Service working_dir remapped under the new project path.
        assert!(
            imported.services[0]
                .working_dir
                .as_deref()
                .unwrap()
                .starts_with(&new_root.join("app").to_string_lossy().to_string()),
        );
        assert!(imported.services[0].env_vars.is_empty());

        // Re-import: everything already registered.
        let report = import_registry(&mut target, &portable, &new_root);
        assert_eq!(report.already_registered, vec!["app"]);
        assert_eq!(target.projects.len(), 1);
    }
}
