use async_trait::async_trait;

use crate::error::Result;
use crate::model::ServiceState;

/// Abstraction over service management.
/// Both ProcessManager (direct mode) and DaemonClient (gRPC mode) implement this.
#[async_trait]
pub trait ServiceBackend: Send + Sync {
    async fn start_all(&self) -> Result<Vec<ServiceState>>;
    async fn start_one(&self, name: &str) -> Result<ServiceState>;
    async fn stop_all(&self) -> Result<()>;
    async fn stop_one(&self, name: &str) -> Result<()>;
    async fn get_states(&self) -> Result<Vec<ServiceState>>;
    async fn get_state(&self, name: &str) -> Result<Option<ServiceState>>;
    async fn refresh_status(&self) -> Result<()>;
}
