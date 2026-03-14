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
    Flutter,
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

/// Docker-specific configuration for a service.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DockerConfig {
    /// Port mappings (e.g., "5432:5432", "8080:80").
    #[serde(default)]
    pub ports: Vec<String>,
    /// Volume mounts (e.g., "./data:/var/lib/postgresql/data").
    #[serde(default)]
    pub volumes: Vec<String>,
    /// Extra docker run arguments (e.g., "--network=host", "--gpus=all").
    #[serde(default)]
    pub extra_args: Vec<String>,
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
    /// Docker-specific config (ports, volumes, extra args). Only used when target = "docker".
    #[serde(default)]
    pub docker: Option<DockerConfig>,
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
    /// Detected URL from stdout (e.g., http://localhost:3000)
    pub url: Option<String>,
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
            url: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_display() {
        assert_eq!(format!("{}", Target::Windows), "windows");
        assert_eq!(format!("{}", Target::Wsl), "wsl");
        assert_eq!(format!("{}", Target::Docker), "docker");
        assert_eq!(format!("{}", Target::Ssh), "ssh");
    }

    #[test]
    fn test_service_status_display() {
        assert_eq!(format!("{}", ServiceStatus::Stopped), "STOPPED");
        assert_eq!(format!("{}", ServiceStatus::Starting), "STARTING");
        assert_eq!(format!("{}", ServiceStatus::Running), "RUNNING");
        assert_eq!(format!("{}", ServiceStatus::Failed), "FAILED");
        assert_eq!(format!("{}", ServiceStatus::Stopping), "STOPPING");
    }

    #[test]
    fn test_service_state_new() {
        let state = ServiceState::new("my-service".to_string());
        assert_eq!(state.service_name, "my-service");
        assert_eq!(state.status, ServiceStatus::Stopped);
        assert!(state.pid.is_none());
        assert!(state.started_at.is_none());
        assert!(state.url.is_none());
        assert!(state.exit_code.is_none());
    }

    #[test]
    fn test_session_id_unique() {
        let a = SessionId::new();
        let b = SessionId::new();
        assert_ne!(a, b, "session IDs should be unique");
        assert_eq!(a.0.len(), 8, "session ID should be 8 chars");
    }

    #[test]
    fn test_session_id_default() {
        let s = SessionId::default();
        assert_eq!(s.0.len(), 8);
    }

    #[test]
    fn test_target_serde_roundtrip() {
        let json = serde_json::to_string(&Target::Docker).unwrap();
        assert_eq!(json, "\"docker\"");
        let parsed: Target = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Target::Docker);
    }

    #[test]
    fn test_project_type_serde_roundtrip() {
        let json = serde_json::to_string(&ProjectType::Rust).unwrap();
        assert_eq!(json, "\"rust\"");
        let parsed: ProjectType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ProjectType::Rust);
    }

    #[test]
    fn test_docker_config_default() {
        let dc = DockerConfig::default();
        assert!(dc.ports.is_empty());
        assert!(dc.volumes.is_empty());
        assert!(dc.extra_args.is_empty());
    }

    #[test]
    fn test_hook_config_default() {
        let hc = HookConfig::default();
        assert!(!hc.venv);
        assert!(!hc.install_deps);
        assert!(!hc.build);
        assert!(hc.custom.is_empty());
    }

    #[test]
    fn test_service_deserialize() {
        let toml = r#"
name = "api"
command = "cargo run"
target = "windows"
"#;
        let svc: Service = toml::from_str(toml).unwrap();
        assert_eq!(svc.name, "api");
        assert_eq!(svc.target, Target::Windows);
        assert!(svc.enabled); // default_true
        assert!(svc.depends_on.is_empty());
    }

    #[test]
    fn test_project_deserialize() {
        let toml = r#"
name = "my-app"
path = "/home/user/my-app"
description = "Test project"

[[services]]
name = "backend"
command = "cargo run"
target = "windows"

[[services]]
name = "frontend"
command = "npm start"
target = "windows"
"#;
        let proj: Project = toml::from_str(toml).unwrap();
        assert_eq!(proj.name, "my-app");
        assert_eq!(proj.services.len(), 2);
        assert_eq!(proj.services[0].name, "backend");
        assert_eq!(proj.services[1].name, "frontend");
    }

    #[test]
    fn test_target_equality() {
        assert_eq!(Target::Windows, Target::Windows);
        assert_ne!(Target::Windows, Target::Docker);
    }

    #[test]
    fn test_service_status_equality() {
        assert_eq!(ServiceStatus::Running, ServiceStatus::Running);
        assert_ne!(ServiceStatus::Running, ServiceStatus::Stopped);
    }
}
