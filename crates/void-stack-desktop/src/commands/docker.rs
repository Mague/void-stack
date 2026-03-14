use serde::Serialize;

use void_stack_core::docker;
use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct DockerAnalysisDto {
    pub has_dockerfile: bool,
    pub has_compose: bool,
    pub dockerfile: Option<DockerfileInfoDto>,
    pub compose: Option<ComposeProjectDto>,
    pub terraform: Vec<InfraResourceDto>,
    pub kubernetes: Vec<K8sResourceDto>,
    pub helm: Option<HelmChartDto>,
}

#[derive(Serialize)]
pub struct DockerfileInfoDto {
    pub stages: Vec<DockerStageDto>,
    pub exposed_ports: Vec<u16>,
    pub entrypoint: Option<String>,
    pub cmd: Option<String>,
    pub workdir: Option<String>,
}

#[derive(Serialize)]
pub struct DockerStageDto {
    pub name: Option<String>,
    pub base_image: String,
}

#[derive(Serialize)]
pub struct ComposeProjectDto {
    pub services: Vec<ComposeServiceDto>,
    pub networks: Vec<String>,
    pub volumes: Vec<String>,
}

#[derive(Serialize)]
pub struct ComposeServiceDto {
    pub name: String,
    pub image: Option<String>,
    pub ports: Vec<PortMappingDto>,
    pub volumes: Vec<VolumeMountDto>,
    pub depends_on: Vec<String>,
    pub kind: String,
    pub has_healthcheck: bool,
}

#[derive(Serialize)]
pub struct PortMappingDto {
    pub host: u16,
    pub container: u16,
}

#[derive(Serialize)]
pub struct VolumeMountDto {
    pub source: String,
    pub target: String,
    pub named: bool,
}

#[derive(Serialize)]
pub struct DockerGenerateResultDto {
    pub dockerfile: Option<String>,
    pub compose: Option<String>,
    pub saved_paths: Vec<String>,
}

#[derive(Serialize)]
pub struct InfraResourceDto {
    pub provider: String,
    pub resource_type: String,
    pub name: String,
    pub kind: String,
    pub details: Vec<String>,
}

#[derive(Serialize)]
pub struct K8sResourceDto {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub images: Vec<String>,
    pub ports: Vec<u16>,
    pub replicas: Option<u32>,
}

#[derive(Serialize)]
pub struct HelmChartDto {
    pub name: String,
    pub version: String,
    pub dependencies: Vec<HelmDependencyDto>,
}

#[derive(Serialize)]
pub struct HelmDependencyDto {
    pub name: String,
    pub version: String,
    pub repository: String,
}

fn analysis_to_dto(a: &docker::DockerAnalysis) -> DockerAnalysisDto {
    DockerAnalysisDto {
        has_dockerfile: a.has_dockerfile,
        has_compose: a.has_compose,
        dockerfile: a.dockerfile.as_ref().map(|df| DockerfileInfoDto {
            stages: df
                .stages
                .iter()
                .map(|s| DockerStageDto {
                    name: s.name.clone(),
                    base_image: s.base_image.clone(),
                })
                .collect(),
            exposed_ports: df.exposed_ports.clone(),
            entrypoint: df.entrypoint.clone(),
            cmd: df.cmd.clone(),
            workdir: df.workdir.clone(),
        }),
        compose: a.compose.as_ref().map(|c| ComposeProjectDto {
            services: c
                .services
                .iter()
                .map(|s| ComposeServiceDto {
                    name: s.name.clone(),
                    image: s.image.clone(),
                    ports: s
                        .ports
                        .iter()
                        .map(|p| PortMappingDto {
                            host: p.host,
                            container: p.container,
                        })
                        .collect(),
                    volumes: s
                        .volumes
                        .iter()
                        .map(|v| VolumeMountDto {
                            source: v.source.clone(),
                            target: v.target.clone(),
                            named: v.named,
                        })
                        .collect(),
                    depends_on: s.depends_on.clone(),
                    kind: format!("{}", s.kind),
                    has_healthcheck: s.healthcheck.is_some(),
                })
                .collect(),
            networks: c.networks.clone(),
            volumes: c.volumes.clone(),
        }),
        terraform: a
            .terraform
            .iter()
            .map(|r| InfraResourceDto {
                provider: r.provider.clone(),
                resource_type: r.resource_type.clone(),
                name: r.name.clone(),
                kind: format!("{}", r.kind),
                details: r.details.clone(),
            })
            .collect(),
        kubernetes: a
            .kubernetes
            .iter()
            .map(|r| K8sResourceDto {
                kind: r.kind.clone(),
                name: r.name.clone(),
                namespace: r.namespace.clone(),
                images: r.images.clone(),
                ports: r.ports.clone(),
                replicas: r.replicas,
            })
            .collect(),
        helm: a.helm.as_ref().map(|h| HelmChartDto {
            name: h.name.clone(),
            version: h.version.clone(),
            dependencies: h
                .dependencies
                .iter()
                .map(|d| HelmDependencyDto {
                    name: d.name.clone(),
                    version: d.version.clone(),
                    repository: d.repository.clone(),
                })
                .collect(),
        }),
    }
}

#[tauri::command]
pub fn docker_analyze(project: String) -> Result<DockerAnalysisDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let clean = strip_win_prefix(&proj.path);
    let path = std::path::Path::new(&clean);

    let analysis = docker::analyze_docker(path);
    Ok(analysis_to_dto(&analysis))
}

#[tauri::command]
pub fn docker_generate(
    project: String,
    generate_dockerfile: bool,
    generate_compose: bool,
    save: bool,
) -> Result<DockerGenerateResultDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let clean = strip_win_prefix(&proj.path);
    let path = std::path::Path::new(&clean);

    let mut result = DockerGenerateResultDto {
        dockerfile: None,
        compose: None,
        saved_paths: Vec::new(),
    };

    if generate_dockerfile {
        let dockerfile_path = path.join("Dockerfile");
        if dockerfile_path.exists() && !save {
            // Show existing Dockerfile content
            let content = std::fs::read_to_string(&dockerfile_path).map_err(|e| e.to_string())?;
            result.dockerfile = Some(content);
        } else {
            // Generate a new Dockerfile
            let pt = void_stack_core::config::detect_project_type(path);
            if let Some(content) = docker::generate_dockerfile::generate(path, pt) {
                if save {
                    std::fs::write(&dockerfile_path, &content).map_err(|e| e.to_string())?;
                    result
                        .saved_paths
                        .push(dockerfile_path.to_string_lossy().to_string());
                    // Also generate .dockerignore if it doesn't exist
                    let dockerignore = path.join(".dockerignore");
                    if !dockerignore.exists() {
                        let ignore = docker::generate_dockerfile::generate_dockerignore(pt);
                        let _ = std::fs::write(&dockerignore, &ignore);
                    }
                }
                result.dockerfile = Some(content);
            }
        }
    }

    if generate_compose {
        let content = docker::generate_compose::generate(&proj, path);
        if save {
            let out = path.join("docker-compose.yml");
            std::fs::write(&out, &content).map_err(|e| e.to_string())?;
            result.saved_paths.push(out.to_string_lossy().to_string());
        }
        result.compose = Some(content);
    }

    Ok(result)
}
