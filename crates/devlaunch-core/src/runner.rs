pub mod local;

use async_trait::async_trait;

use crate::error::Result;
use crate::model::{Service, ServiceState, Target};

/// Trait for all runners (local, docker, ssh, cloud).
#[async_trait]
pub trait Runner: Send + Sync {
    /// Which target this runner handles.
    fn target(&self) -> Target;

    /// Start a service process.
    async fn start(&self, service: &Service, project_path: &str) -> Result<ServiceState>;

    /// Stop a running service.
    async fn stop(&self, service: &Service, pid: u32) -> Result<()>;

    /// Check if a process is still alive.
    async fn is_running(&self, pid: u32) -> Result<bool>;
}

/// Select the right runner for a service target.
pub fn runner_for(target: Target) -> Box<dyn Runner> {
    match target {
        Target::Windows => Box::new(local::LocalRunner::new(Target::Windows)),
        Target::Wsl => Box::new(local::LocalRunner::new(Target::Wsl)),
        // Docker and SSH runners will be added in later phases
        Target::Docker => Box::new(local::LocalRunner::new(Target::Windows)),
        Target::Ssh => Box::new(local::LocalRunner::new(Target::Windows)),
    }
}
