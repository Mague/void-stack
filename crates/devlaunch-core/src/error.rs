use thiserror::Error;

#[derive(Debug, Error)]
pub enum DevLaunchError {
    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Service not found: {service} in project {project}")]
    ServiceNotFound { project: String, service: String },

    #[error("Config file not found: {0}")]
    ConfigNotFound(String),

    #[error("Invalid config: {0}")]
    InvalidConfig(String),

    #[error("Runner error: {0}")]
    RunnerError(String),

    #[error("Process failed to start: {0}")]
    ProcessStartFailed(String),

    #[error("Hook failed: {hook} — {reason}")]
    HookFailed { hook: String, reason: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    TomlParse(#[from] toml::de::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, DevLaunchError>;
