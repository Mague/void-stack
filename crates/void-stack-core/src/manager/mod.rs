mod logs;
mod process;
mod state;
mod url;

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::backend::ServiceBackend;
use crate::error::Result;
use crate::model::{Project, ServiceState};

/// Manages the lifecycle of all services in a project.
pub struct ProcessManager {
    project: Project,
    pub(crate) states: Arc<Mutex<HashMap<String, ServiceState>>>,
    pub(crate) logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// Per-service exit notification, fed by the task that owns the
    /// `tokio::process::Child` and awaits `child.wait()`. Lets stop_one /
    /// stop_all wait for actual process exit instead of sleeping and
    /// re-checking PIDs (which is racy under PID reuse). Absent for
    /// processes the manager didn't spawn (e.g. adopted after a daemon
    /// restart) — those fall back to PID polling.
    pub(crate) exit_watchers: Arc<Mutex<HashMap<String, tokio::sync::watch::Receiver<bool>>>>,
}

impl ProcessManager {
    pub fn new(project: Project) -> Self {
        let states: HashMap<String, ServiceState> = project
            .services
            .iter()
            .map(|s| (s.name.clone(), ServiceState::new(s.name.clone())))
            .collect();

        let logs: HashMap<String, Vec<String>> = project
            .services
            .iter()
            .map(|s| (s.name.clone(), Vec::new()))
            .collect();

        Self {
            project,
            states: Arc::new(Mutex::new(states)),
            logs: Arc::new(Mutex::new(logs)),
            exit_watchers: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ServiceBackend for ProcessManager {
    async fn start_all(&self) -> Result<Vec<ServiceState>> {
        self.start_all().await
    }

    async fn start_one(&self, name: &str) -> Result<ServiceState> {
        self.start_one(name).await
    }

    async fn stop_all(&self) -> Result<()> {
        self.stop_all().await
    }

    async fn stop_one(&self, name: &str) -> Result<()> {
        self.stop_one(name).await
    }

    async fn get_states(&self) -> Result<Vec<ServiceState>> {
        Ok(ProcessManager::get_states(self).await)
    }

    async fn get_state(&self, name: &str) -> Result<ServiceState> {
        ProcessManager::get_state(self, name).await
    }

    async fn refresh_status(&self) -> Result<()> {
        ProcessManager::refresh_status(self).await
    }

    async fn get_logs(&self, name: &str) -> Result<Vec<String>> {
        Ok(ProcessManager::get_logs(self, name).await)
    }
}
