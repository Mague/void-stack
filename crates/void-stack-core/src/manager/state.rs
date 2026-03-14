use super::ProcessManager;
use crate::error::Result;
use crate::model::{Project, ServiceState, ServiceStatus};
use crate::runner;

impl ProcessManager {
    /// Get current state of all services.
    pub async fn get_states(&self) -> Vec<ServiceState> {
        let states = self.states.lock().await;
        states.values().cloned().collect()
    }

    /// Get state of a single service.
    pub async fn get_state(&self, name: &str) -> Option<ServiceState> {
        let states = self.states.lock().await;
        states.get(name).cloned()
    }

    /// Get captured logs for a service.
    pub async fn get_logs(&self, name: &str) -> Vec<String> {
        let logs = self.logs.lock().await;
        logs.get(name).cloned().unwrap_or_default()
    }

    /// Refresh the running status by checking PIDs.
    pub async fn refresh_status(&self) -> Result<()> {
        let mut states = self.states.lock().await;

        for (name, state) in states.iter_mut() {
            if state.status == ServiceStatus::Running
                && let Some(pid) = state.pid
            {
                let service = self.project.services.iter().find(|s| s.name == *name);
                if let Some(service) = service {
                    let runner = runner::runner_for(service.target);
                    let running = runner.is_running(pid).await.unwrap_or(false);
                    if !running {
                        state.status = ServiceStatus::Stopped;
                        state.pid = None;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn project(&self) -> &Project {
        &self.project
    }
}
