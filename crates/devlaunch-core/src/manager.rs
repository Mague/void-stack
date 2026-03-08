use std::collections::HashMap;
use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::backend::ServiceBackend;
use crate::error::{DevLaunchError, Result};
use crate::hooks;
use crate::model::{Project, ServiceState, ServiceStatus};
use crate::runner;

/// Manages the lifecycle of all services in a project.
pub struct ProcessManager {
    project: Project,
    states: Mutex<HashMap<String, ServiceState>>,
}

impl ProcessManager {
    pub fn new(project: Project) -> Self {
        let states: HashMap<String, ServiceState> = project
            .services
            .iter()
            .map(|s| (s.name.clone(), ServiceState::new(s.name.clone())))
            .collect();

        Self {
            project,
            states: Mutex::new(states),
        }
    }

    /// Start all enabled services in parallel.
    pub async fn start_all(&self) -> Result<Vec<ServiceState>> {
        let enabled: Vec<_> = self
            .project
            .services
            .iter()
            .filter(|s| s.enabled)
            .collect();

        info!(count = enabled.len(), "Starting all enabled services");

        // Run pre-launch hooks if configured
        if let Some(hook_config) = &self.project.hooks {
            hooks::run_pre_launch(
                hook_config,
                &self.project.path,
                self.project.project_type,
            )
            .await?;
        }

        let mut results = Vec::new();

        for service in &enabled {
            let runner = runner::runner_for(service.target);
            match runner.start(service, &self.project.path).await {
                Ok(state) => {
                    let mut states = self.states.lock().await;
                    states.insert(service.name.clone(), state.clone());
                    results.push(state);
                }
                Err(e) => {
                    error!(service = %service.name, error = %e, "Failed to start");
                    let mut state = ServiceState::new(service.name.clone());
                    state.status = ServiceStatus::Failed;
                    state.last_log_line = Some(e.to_string());

                    let mut states = self.states.lock().await;
                    states.insert(service.name.clone(), state.clone());
                    results.push(state);
                }
            }
        }

        Ok(results)
    }

    /// Start a single service by name.
    pub async fn start_one(&self, name: &str) -> Result<ServiceState> {
        let service = self
            .project
            .services
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| DevLaunchError::ServiceNotFound {
                project: self.project.name.clone(),
                service: name.to_string(),
            })?;

        let runner = runner::runner_for(service.target);
        let state = runner.start(service, &self.project.path).await?;

        let mut states = self.states.lock().await;
        states.insert(name.to_string(), state.clone());
        Ok(state)
    }

    /// Stop all running services.
    pub async fn stop_all(&self) -> Result<()> {
        info!("Stopping all services");
        let states = self.states.lock().await;

        for (name, state) in states.iter() {
            if state.status == ServiceStatus::Running {
                if let Some(pid) = state.pid {
                    let service = self
                        .project
                        .services
                        .iter()
                        .find(|s| s.name == *name);

                    if let Some(service) = service {
                        let runner = runner::runner_for(service.target);
                        if let Err(e) = runner.stop(service, pid).await {
                            error!(service = %name, error = %e, "Failed to stop");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Stop a single service by name.
    pub async fn stop_one(&self, name: &str) -> Result<()> {
        let states = self.states.lock().await;
        let state = states.get(name).ok_or_else(|| DevLaunchError::ServiceNotFound {
            project: self.project.name.clone(),
            service: name.to_string(),
        })?;

        if let Some(pid) = state.pid {
            let service = self
                .project
                .services
                .iter()
                .find(|s| s.name == name)
                .ok_or_else(|| DevLaunchError::ServiceNotFound {
                    project: self.project.name.clone(),
                    service: name.to_string(),
                })?;

            let runner = runner::runner_for(service.target);
            runner.stop(service, pid).await?;
        }

        Ok(())
    }

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

    /// Refresh the running status by checking PIDs.
    pub async fn refresh_status(&self) -> Result<()> {
        let mut states = self.states.lock().await;

        for (name, state) in states.iter_mut() {
            if state.status == ServiceStatus::Running {
                if let Some(pid) = state.pid {
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
        }

        Ok(())
    }

    pub fn project(&self) -> &Project {
        &self.project
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
}
