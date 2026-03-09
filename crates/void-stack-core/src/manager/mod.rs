mod logs;
mod process;
mod state;
mod url;

use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use tokio::process::Child;
use tokio::sync::Mutex;

use crate::backend::ServiceBackend;
use crate::error::Result;
use crate::model::{Project, ServiceState};

/// Manages the lifecycle of all services in a project.
pub struct ProcessManager {
    project: Project,
    pub(crate) states: Arc<Mutex<HashMap<String, ServiceState>>>,
    pub(crate) children: Mutex<HashMap<String, Child>>,
    pub(crate) logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
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
            children: Mutex::new(HashMap::new()),
            logs: Arc::new(Mutex::new(logs)),
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

    async fn get_state(&self, name: &str) -> Result<Option<ServiceState>> {
        Ok(ProcessManager::get_state(self, name).await)
    }

    async fn refresh_status(&self) -> Result<()> {
        ProcessManager::refresh_status(self).await
    }

    async fn get_logs(&self, name: &str) -> Result<Vec<String>> {
        Ok(ProcessManager::get_logs(self, name).await)
    }
}
