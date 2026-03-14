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
        _ => Target::Windows,
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
        .unwrap();

    proj_mut
        .services
        .retain(|s| !s.name.eq_ignore_ascii_case(&docker_name));
    for svc in to_import {
        proj_mut.services.push(svc);
    }

    save_global_config(&config).map_err(|e| e.to_string())?;
    Ok(count)
}
