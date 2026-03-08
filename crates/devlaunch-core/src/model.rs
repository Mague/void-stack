use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where a service runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Windows,
    Wsl,
    Docker,
    Ssh,
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Windows => write!(f, "windows"),
            Target::Wsl => write!(f, "wsl"),
            Target::Docker => write!(f, "docker"),
            Target::Ssh => write!(f, "ssh"),
        }
    }
}

/// Type of project for auto-detection and pre-launch hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    Python,
    Node,
    Rust,
    Go,
    Docker,
    Unknown,
}

/// Runtime status of a managed process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Failed,
    Stopping,
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceStatus::Stopped => write!(f, "STOPPED"),
            ServiceStatus::Starting => write!(f, "STARTING"),
            ServiceStatus::Running => write!(f, "RUNNING"),
            ServiceStatus::Failed => write!(f, "FAILED"),
            ServiceStatus::Stopping => write!(f, "STOPPING"),
        }
    }
}

/// A single service within a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub command: String,
    pub target: Target,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub env_vars: Vec<(String, String)>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// Pre-launch hook configuration for a service.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookConfig {
    #[serde(default)]
    pub venv: bool,
    #[serde(default)]
    pub install_deps: bool,
    #[serde(default)]
    pub build: bool,
    #[serde(default)]
    pub custom: Vec<String>,
}

/// A project with its services.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub path: String,
    #[serde(default)]
    pub project_type: Option<ProjectType>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub services: Vec<Service>,
    #[serde(default)]
    pub hooks: Option<HookConfig>,
}

/// Live state of a running service (not persisted, runtime only).
#[derive(Debug, Clone)]
pub struct ServiceState {
    pub service_name: String,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub started_at: Option<DateTime<Utc>>,
    pub cpu_percent: Option<f32>,
    pub memory_mb: Option<f64>,
    pub last_log_line: Option<String>,
    pub exit_code: Option<i32>,
}

impl ServiceState {
    pub fn new(service_name: String) -> Self {
        Self {
            service_name,
            status: ServiceStatus::Stopped,
            pid: None,
            started_at: None,
            cpu_percent: None,
            memory_mb: None,
            last_log_line: None,
            exit_code: None,
        }
    }
}

/// Unique identifier for a running project session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string()[..8].to_string())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

fn default_true() -> bool {
    true
}
