use serde::Serialize;
use std::path::Path;

use void_stack_core::config::detect_project_type;
use void_stack_core::docker;
use void_stack_core::global_config::{
    default_command_for_dir, load_global_config, remove_service, save_global_config,
    scan_subprojects,
};
use void_stack_core::model::{DockerConfig, Target};
use void_stack_core::runner::local::strip_win_prefix;

#[derive(Serialize)]
pub struct ScanResultDto {
    pub services: Vec<ScannedServiceDto>,
    pub project_type: String,
}

#[derive(Serialize)]
pub struct ScannedServiceDto {
    pub name: String,
    pub command: String,
    pub working_dir: String,
    pub detected_type: String,
}

/// Scan a directory without registering it — preview what would be detected.
#[tauri::command]
pub fn scan_directory(path: String) -> Result<ScanResultDto, String> {
    let clean = strip_win_prefix(&path);
    let scan_path = std::path::Path::new(&clean);

    if !scan_path.exists() {
        return Err(format!("La ruta '{}' no existe", path));
    }

    let subs = scan_subprojects(scan_path);
    let project_type = detect_project_type(scan_path);

    let services: Vec<ScannedServiceDto> = if subs.is_empty() {
        let cmd = default_command_for_dir(project_type, scan_path);
        vec![ScannedServiceDto {
            name: scan_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "default".to_string()),
            command: cmd,
            working_dir: path.clone(),
            detected_type: format!("{:?}", project_type),
        }]
    } else {
        subs.into_iter()
            .map(|(name, svc_path, svc_type)| {
                let cmd = default_command_for_dir(svc_type, &svc_path);
                ScannedServiceDto {
                    name,
                    command: cmd,
                    working_dir: svc_path.to_string_lossy().to_string(),
                    detected_type: format!("{:?}", svc_type),
                }
            })
            .collect()
    };

    Ok(ScanResultDto {
        services,
        project_type: format!("{:?}", project_type),
    })
}

/// Manually add a service to an existing project.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn add_service_cmd(
    project: String,
    name: String,
    command: String,
    working_dir: String,
    target: Option<String>,
    docker_ports: Option<Vec<String>>,
    docker_volumes: Option<Vec<String>>,
    docker_extra_args: Option<Vec<String>>,
) -> Result<bool, String> {
    let mut config = load_global_config().map_err(|e| e.to_string())?;
    let proj = config
        .projects
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(&project))
        .ok_or_else(|| format!("Proyecto '{}' no encontrado", project))?;

    if proj
        .services
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&name))
    {
        return Err(format!("El servicio '{}' ya existe en '{}'", name, project));
    }

    let tgt = match target.as_deref() {
        Some("wsl") | Some("WSL") => Target::Wsl,
        Some("docker") | Some("Docker") => Target::Docker,
        Some("macos") | Some("MacOS") => Target::MacOS,
        _ => Target::native(),
    };

    let docker = if tgt == Target::Docker {
        let ports = docker_ports.unwrap_or_default();
        let volumes = docker_volumes.unwrap_or_default();
        let extra_args = docker_extra_args.unwrap_or_default();
        if !ports.is_empty() || !volumes.is_empty() || !extra_args.is_empty() {
            Some(DockerConfig {
                ports,
                volumes,
                extra_args,
            })
        } else {
            None
        }
    } else {
        None
    };

    proj.services.push(void_stack_core::model::Service {
        name,
        command,
        target: tgt,
        working_dir: Some(working_dir),
        enabled: true,
        env_vars: Vec::new(),
        depends_on: Vec::new(),
        docker,
    });

    save_global_config(&config).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Remove a service from a project.
#[tauri::command]
pub fn remove_service_cmd(project: String, service: String) -> Result<bool, String> {
    let mut config = load_global_config().map_err(|e| e.to_string())?;
    if !remove_service(&mut config, &project, &service) {
        return Err(format!(
            "Servicio '{}' no encontrado en '{}'",
            service, project
        ));
    }
    save_global_config(&config).map_err(|e| e.to_string())?;
    Ok(true)
}

/// DTO for a Docker service detected from docker-compose.yml or Dockerfile.
#[derive(Serialize)]
pub struct DockerServicePreview {
    pub name: String,
    pub image: Option<String>,
    pub ports: Vec<String>,
    pub volumes: Vec<String>,
    pub env_vars: Vec<(String, String)>,
    pub depends_on: Vec<String>,
    pub kind: String,
    pub source: String, // "compose" or "dockerfile"
    pub already_exists: bool,
}

/// Scan project for docker-compose.yml / Dockerfile and return a preview.
/// For compose: returns ONE entry representing the entire stack (all containers together).
/// For Dockerfile: returns ONE entry for the single container.
#[tauri::command]
pub fn detect_docker_services(project: String) -> Result<Vec<DockerServicePreview>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = config
        .projects
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(&project))
        .ok_or_else(|| format!("Proyecto '{}' no encontrado", project))?;

    let existing_names: Vec<String> = proj
        .services
        .iter()
        .map(|s| s.name.to_lowercase())
        .collect();

    let project_path = Path::new(&proj.path);
    let mut previews = Vec::new();

    // Check for docker-compose.yml → ONE service that runs `docker compose up`
    if let Some(compose_path) = docker::parse::find_compose_file(project_path)
        && let Some(compose) = docker::parse::parse_compose(&compose_path)
    {
        // Aggregate all ports, volumes, env_vars, depends_on across all compose services
        let mut all_ports = Vec::new();
        let mut all_volumes = Vec::new();
        let mut all_env = Vec::new();
        let mut sub_names = Vec::new();

        for svc in &compose.services {
            sub_names.push(svc.name.clone());
            for p in &svc.ports {
                all_ports.push(format!("{}:{}", p.host, p.container));
            }
            for v in &svc.volumes {
                all_volumes.push(format!("{}:{}", v.source, v.target));
            }
            all_env.extend(svc.env_vars.clone());
        }

        let docker_name = format!("docker:{}", proj.name);
        let compose_file = compose_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "docker-compose.yml".to_string());

        previews.push(DockerServicePreview {
            name: docker_name.clone(),
            image: Some(format!(
                "{} ({} containers: {})",
                compose_file,
                sub_names.len(),
                sub_names.join(", ")
            )),
            ports: all_ports,
            volumes: all_volumes,
            env_vars: all_env,
            depends_on: sub_names,
            kind: "compose".to_string(),
            source: "compose".to_string(),
            already_exists: existing_names.contains(&docker_name.to_lowercase()),
        });
    }

    // If no compose, check for Dockerfile → ONE service
    if previews.is_empty() {
        let dockerfile_path = project_path.join("Dockerfile");
        if let Some(info) = docker::parse::parse_dockerfile(&dockerfile_path) {
            let ports: Vec<String> = info
                .exposed_ports
                .iter()
                .map(|p| format!("{}:{}", p, p))
                .collect();
            let docker_name = format!("docker:{}", proj.name);
            previews.push(DockerServicePreview {
                name: docker_name.clone(),
                image: Some(
                    info.stages
                        .last()
                        .map(|s| s.base_image.clone())
                        .unwrap_or_default(),
                ),
                ports,
                volumes: Vec::new(),
                env_vars: info.env_vars,
                depends_on: Vec::new(),
                kind: "app".to_string(),
                source: "dockerfile".to_string(),
                already_exists: existing_names.contains(&docker_name.to_lowercase()),
            });
        }
    }

    Ok(previews)
}

/// Import a Docker stack into a project as a single service.
/// Compose → `docker compose up` (starts all containers).
/// Dockerfile → `docker build + run`.
/// Replaces existing service with same name if present.
#[tauri::command]
pub fn import_docker_services(
    project: String,
    service_names: Vec<String>,
) -> Result<usize, String> {
    let mut config = load_global_config().map_err(|e| e.to_string())?;

    let proj = config
        .projects
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(&project))
        .ok_or_else(|| format!("Proyecto '{}' no encontrado", project))?;

    let project_path = Path::new(&proj.path).to_path_buf();
    let docker_name = format!("docker:{}", proj.name);

    // Only proceed if the user selected this service
    if !service_names
        .iter()
        .any(|n| n.eq_ignore_ascii_case(&docker_name))
    {
        return Ok(0);
    }

    let mut to_import = Vec::new();

    if let Some(compose_path) = docker::parse::find_compose_file(&project_path) {
        // Compose: single service that runs `docker compose up`
        // No need for -f flag — working_dir is set to project path, compose finds the file automatically
        let command = "docker compose up".to_string();

        // Aggregate all ports for DockerConfig display
        let mut all_ports = Vec::new();
        let mut all_volumes = Vec::new();
        if let Some(compose) = docker::parse::parse_compose(&compose_path) {
            for svc in &compose.services {
                for p in &svc.ports {
                    all_ports.push(format!("{}:{}", p.host, p.container));
                }
                for v in &svc.volumes {
                    all_volumes.push(format!("{}:{}", v.source, v.target));
                }
            }
        }

        let docker_config = if !all_ports.is_empty() || !all_volumes.is_empty() {
            Some(DockerConfig {
                ports: all_ports,
                volumes: all_volumes,
                extra_args: Vec::new(),
            })
        } else {
            None
        };

        to_import.push(void_stack_core::model::Service {
            name: docker_name.clone(),
            command,
            target: Target::Docker,
            working_dir: Some(project_path.to_string_lossy().to_string()),
            enabled: true,
            env_vars: Vec::new(),
            depends_on: Vec::new(),
            docker: docker_config,
        });
    } else {
        // Dockerfile: single service that builds + runs
        let dockerfile_path = project_path.join("Dockerfile");
        if let Some(info) = docker::parse::parse_dockerfile(&dockerfile_path) {
            let ports: Vec<String> = info
                .exposed_ports
                .iter()
                .map(|p| format!("{}:{}", p, p))
                .collect();
            let cmd = info.cmd.or(info.entrypoint).unwrap_or_default();
            let docker_config = if !ports.is_empty() {
                Some(DockerConfig {
                    ports,
                    volumes: Vec::new(),
                    extra_args: Vec::new(),
                })
            } else {
                None
            };
            to_import.push(void_stack_core::model::Service {
                name: docker_name.clone(),
                command: cmd,
                target: Target::Docker,
                working_dir: Some(project_path.to_string_lossy().to_string()),
                enabled: true,
                env_vars: info.env_vars,
                depends_on: Vec::new(),
                docker: docker_config,
            });
        }
    }

    let count = to_import.len();
    if count == 0 {
        return Ok(0);
    }

    // Re-find project mutably, remove existing service with same name, then add
    let proj_mut = config
        .projects
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(&project))
        .ok_or_else(|| format!("project '{}' disappeared while importing services", project))?;

    proj_mut
        .services
        .retain(|s| !s.name.eq_ignore_ascii_case(&docker_name));
    for svc in to_import {
        proj_mut.services.push(svc);
    }

    save_global_config(&config).map_err(|e| e.to_string())?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support;

    #[test]
    fn test_scan_directory_missing_path_errors() {
        assert!(scan_directory("Z:/no/such/place".to_string()).is_err());
    }

    #[test]
    fn test_scan_directory_detects_rust_single_service() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();

        let result = scan_directory(dir.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(result.project_type, "Rust");
        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].detected_type, "Rust");
    }

    #[test]
    fn test_add_and_remove_service_cmd() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Svc", dir.path()));

        // Add a native service.
        let ok = add_service_cmd(
            "Svc".to_string(),
            "web".to_string(),
            "npm run dev".to_string(),
            dir.path().to_string_lossy().to_string(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(ok);

        // Duplicate service name is rejected.
        let dup = add_service_cmd(
            "Svc".to_string(),
            "WEB".to_string(),
            "x".to_string(),
            String::new(),
            None,
            None,
            None,
            None,
        );
        assert!(dup.is_err());

        // Remove it.
        assert!(remove_service_cmd("Svc".to_string(), "web".to_string()).unwrap());
        // Removing again → not found error.
        assert!(remove_service_cmd("Svc".to_string(), "web".to_string()).is_err());
    }

    #[test]
    fn test_add_service_cmd_unknown_project_errors() {
        let _g = test_support::config_guard();
        let err = add_service_cmd(
            "Ghost".to_string(),
            "s".to_string(),
            "cmd".to_string(),
            String::new(),
            None,
            None,
            None,
            None,
        );
        assert!(err.is_err());
    }

    #[test]
    fn test_add_service_cmd_docker_builds_config() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Dk", dir.path()));

        add_service_cmd(
            "Dk".to_string(),
            "db".to_string(),
            "docker compose up".to_string(),
            dir.path().to_string_lossy().to_string(),
            Some("docker".to_string()),
            Some(vec!["5432:5432".to_string()]),
            None,
            None,
        )
        .unwrap();

        let cfg = load_global_config().unwrap();
        let proj = cfg.projects.iter().find(|p| p.name == "Dk").unwrap();
        let s = proj.services.iter().find(|s| s.name == "db").unwrap();
        assert_eq!(s.target, Target::Docker);
        let docker = s.docker.as_ref().unwrap();
        assert_eq!(docker.ports, vec!["5432:5432".to_string()]);
    }

    /// Write a two-service docker-compose.yml into `dir`.
    fn write_compose(dir: &std::path::Path) {
        let compose = r#"
services:
  web:
    image: nginx
    ports:
      - "8080:80"
  db:
    image: postgres
    ports:
      - "5432:5432"
    volumes:
      - "./data:/var/lib/postgresql/data"
"#;
        std::fs::write(dir.join("docker-compose.yml"), compose).unwrap();
    }

    #[test]
    fn test_detect_docker_services_compose() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        write_compose(dir.path());
        test_support::register(test_support::project("Compose", dir.path()));

        let previews = detect_docker_services("Compose".to_string()).unwrap();
        assert_eq!(previews.len(), 1);
        let p = &previews[0];
        assert_eq!(p.source, "compose");
        assert_eq!(p.kind, "compose");
        assert_eq!(p.name, "docker:Compose");
        // Ports aggregated across both containers.
        assert!(p.ports.contains(&"8080:80".to_string()));
        assert!(p.ports.contains(&"5432:5432".to_string()));
        assert_eq!(p.depends_on.len(), 2);
        assert!(!p.already_exists);
    }

    #[test]
    fn test_detect_docker_services_dockerfile_only() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Dockerfile"),
            "FROM node:20\nEXPOSE 3000\nCMD [\"node\", \"index.js\"]\n",
        )
        .unwrap();
        test_support::register(test_support::project("Df", dir.path()));

        let previews = detect_docker_services("Df".to_string()).unwrap();
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source, "dockerfile");
        assert!(previews[0].ports.contains(&"3000:3000".to_string()));
    }

    #[test]
    fn test_detect_docker_services_unknown_project_errors() {
        let _g = test_support::config_guard();
        assert!(detect_docker_services("Ghost".to_string()).is_err());
    }

    #[test]
    fn test_import_docker_services_compose() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        write_compose(dir.path());
        test_support::register(test_support::project("Imp", dir.path()));

        // Not selecting the docker service imports nothing.
        let none = import_docker_services("Imp".to_string(), vec!["other".to_string()]).unwrap();
        assert_eq!(none, 0);

        // Selecting docker:<proj> imports one aggregated service.
        let count =
            import_docker_services("Imp".to_string(), vec!["docker:Imp".to_string()]).unwrap();
        assert_eq!(count, 1);

        let cfg = load_global_config().unwrap();
        let proj = cfg.projects.iter().find(|p| p.name == "Imp").unwrap();
        let s = proj
            .services
            .iter()
            .find(|s| s.name == "docker:Imp")
            .unwrap();
        assert_eq!(s.command, "docker compose up");
        assert_eq!(s.target, Target::Docker);
    }
}
