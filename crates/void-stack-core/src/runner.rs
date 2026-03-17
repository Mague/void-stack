pub mod docker;
pub mod local;

use async_trait::async_trait;
use tokio::process::Child;

use crate::error::Result;
use crate::model::{Service, ServiceState, Target};

/// Result of starting a service: the state info + the child process handle.
pub struct StartResult {
    pub state: ServiceState,
    pub child: Child,
}

/// Trait for all runners (local, docker, ssh, cloud).
#[async_trait]
pub trait Runner: Send + Sync {
    /// Which target this runner handles.
    fn target(&self) -> Target;

    /// Start a service process. Returns state + child handle for log capture.
    async fn start(&self, service: &Service, project_path: &str) -> Result<StartResult>;

    /// Stop a running service.
    async fn stop(&self, service: &Service, pid: u32) -> Result<()>;

    /// Check if a process is still alive.
    async fn is_running(&self, pid: u32) -> Result<bool>;
}

/// Select the right runner for a service target.
pub fn runner_for(target: Target) -> Box<dyn Runner> {
    match target {
        Target::Windows | Target::MacOS => Box::new(local::LocalRunner::new(target)),
        Target::Wsl => Box::new(local::LocalRunner::new(Target::Wsl)),
        Target::Docker => Box::new(docker::DockerRunner::new()),
        // SSH runner will be added in a later phase
        Target::Ssh => Box::new(local::LocalRunner::new(Target::native())),
    }
}
