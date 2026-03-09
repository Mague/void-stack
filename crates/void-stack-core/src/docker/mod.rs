//! Docker & Infrastructure Intelligence — parse, analyze, and generate Docker,
//! Terraform, Kubernetes, and Helm artifacts.

pub mod parse;
pub mod generate_dockerfile;
pub mod generate_compose;
pub mod terraform;
pub mod kubernetes;
pub mod helm;

use std::path::Path;
use serde::Serialize;

// ── Core types ──

/// Classification of a compose service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComposeServiceKind {
    App,
    Database,
    Cache,
    Proxy,
    Queue,
    Worker,
    Unknown,
}

impl std::fmt::Display for ComposeServiceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComposeServiceKind::App => write!(f, "app"),
            ComposeServiceKind::Database => write!(f, "database"),
            ComposeServiceKind::Cache => write!(f, "cache"),
            ComposeServiceKind::Proxy => write!(f, "proxy"),
            ComposeServiceKind::Queue => write!(f, "queue"),
            ComposeServiceKind::Worker => write!(f, "worker"),
            ComposeServiceKind::Unknown => write!(f, "unknown"),
        }
    }
}

/// Port mapping from host to container.
#[derive(Debug, Clone, Serialize)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
}

/// Volume mount.
#[derive(Debug, Clone, Serialize)]
pub struct VolumeMount {
    pub source: String,
    pub target: String,
    pub named: bool,
}

/// Healthcheck configuration.
#[derive(Debug, Clone, Serialize)]
pub struct HealthCheck {
    pub test: String,
    pub interval: Option<String>,
    pub timeout: Option<String>,
    pub retries: Option<u32>,
}

/// Build configuration for a compose service.
#[derive(Debug, Clone, Serialize)]
pub struct ComposeBuild {
    pub context: String,
    pub dockerfile: Option<String>,
    pub target: Option<String>,
}

/// A single service from docker-compose.
#[derive(Debug, Clone, Serialize)]
pub struct ComposeService {
    pub name: String,
    pub image: Option<String>,
    pub build: Option<ComposeBuild>,
    pub ports: Vec<PortMapping>,
    pub volumes: Vec<VolumeMount>,
    pub env_vars: Vec<(String, String)>,
    pub depends_on: Vec<String>,
    pub healthcheck: Option<HealthCheck>,
    pub kind: ComposeServiceKind,
}

/// A parsed docker-compose project.
#[derive(Debug, Clone, Serialize)]
pub struct ComposeProject {
    pub services: Vec<ComposeService>,
    pub networks: Vec<String>,
    pub volumes: Vec<String>,
}

/// A single stage in a multi-stage Dockerfile.
#[derive(Debug, Clone, Serialize)]
pub struct DockerStage {
    pub name: Option<String>,
    pub base_image: String,
}

/// Parsed Dockerfile metadata.
#[derive(Debug, Clone, Serialize)]
pub struct DockerfileInfo {
    pub stages: Vec<DockerStage>,
    pub exposed_ports: Vec<u16>,
    pub entrypoint: Option<String>,
    pub cmd: Option<String>,
    pub env_vars: Vec<(String, String)>,
    pub workdir: Option<String>,
}

// ── Infrastructure types ──

/// Classification of an infrastructure resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InfraResourceKind {
    Database,
    Cache,
    Storage,
    Compute,
    Queue,
    Network,
    Other,
}

impl std::fmt::Display for InfraResourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InfraResourceKind::Database => write!(f, "database"),
            InfraResourceKind::Cache => write!(f, "cache"),
            InfraResourceKind::Storage => write!(f, "storage"),
            InfraResourceKind::Compute => write!(f, "compute"),
            InfraResourceKind::Queue => write!(f, "queue"),
            InfraResourceKind::Network => write!(f, "network"),
            InfraResourceKind::Other => write!(f, "other"),
        }
    }
}

/// A Terraform infrastructure resource.
#[derive(Debug, Clone, Serialize)]
pub struct InfraResource {
    pub provider: String,
    pub resource_type: String,
    pub name: String,
    pub kind: InfraResourceKind,
    pub details: Vec<String>,
}

/// A Kubernetes resource parsed from YAML manifests.
#[derive(Debug, Clone, Serialize)]
pub struct K8sResource {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub images: Vec<String>,
    pub ports: Vec<u16>,
    pub replicas: Option<u32>,
}

/// A parsed Helm chart.
#[derive(Debug, Clone, Serialize)]
pub struct HelmChart {
    pub name: String,
    pub version: String,
    pub dependencies: Vec<HelmDependency>,
}

/// A Helm chart dependency.
#[derive(Debug, Clone, Serialize)]
pub struct HelmDependency {
    pub name: String,
    pub version: String,
    pub repository: String,
}

/// Full Docker & infrastructure analysis for a project.
#[derive(Debug, Clone, Serialize)]
pub struct DockerAnalysis {
    pub has_dockerfile: bool,
    pub has_compose: bool,
    pub dockerfile: Option<DockerfileInfo>,
    pub compose: Option<ComposeProject>,
    pub terraform: Vec<InfraResource>,
    pub kubernetes: Vec<K8sResource>,
    pub helm: Option<HelmChart>,
}

/// Result of Docker file generation.
#[derive(Debug, Clone, Serialize)]
pub struct DockerGenerateResult {
    pub dockerfile: Option<String>,
    pub compose: Option<String>,
    pub saved_paths: Vec<String>,
}

// ── Top-level API ──

/// Analyze existing Docker and infrastructure artifacts in a project directory.
pub fn analyze_docker(project_path: &Path) -> DockerAnalysis {
    let dockerfile_path = project_path.join("Dockerfile");
    let dockerfile = if dockerfile_path.exists() {
        parse::parse_dockerfile(&dockerfile_path)
    } else {
        None
    };

    let compose = parse::find_compose_file(project_path)
        .and_then(|p| parse::parse_compose(&p));

    let tf_resources = terraform::parse_terraform(project_path);
    let k8s_resources = kubernetes::parse_kubernetes(project_path);
    let helm_chart = helm::parse_helm(project_path);

    DockerAnalysis {
        has_dockerfile: dockerfile.is_some(),
        has_compose: compose.is_some(),
        dockerfile,
        compose,
        terraform: tf_resources,
        kubernetes: k8s_resources,
        helm: helm_chart,
    }
}
