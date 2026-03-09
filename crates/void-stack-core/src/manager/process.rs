use std::sync::Arc;
use tracing::{error, info};

use crate::error::{VoidStackError, Result};
use crate::hooks;
use crate::model::{ServiceState, ServiceStatus};
use crate::runner;
use super::ProcessManager;

impl ProcessManager {
    /// Check if a service is currently running (has a live PID).
    pub(crate) async fn is_service_running(&self, name: &str) -> bool {
        let states = self.states.lock().await;
        if let Some(state) = states.get(name) {
            if state.status == ServiceStatus::Running {
                if let Some(pid) = state.pid {
                    // Verify the PID is actually alive
                    let service = self.project.services.iter().find(|s| s.name == name);
                    if let Some(service) = service {
                        let runner = runner::runner_for(service.target);
                        return runner.is_running(pid).await.unwrap_or(false);
                    }
                }
            }
        }
        false
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
            // Skip services that are already running
            if self.is_service_running(&service.name).await {
                info!(service = %service.name, "Already running — skipping");
                let states = self.states.lock().await;
                if let Some(state) = states.get(&service.name) {
                    results.push(state.clone());
                }
                continue;
            }

            let runner = runner::runner_for(service.target);
            match runner.start(service, &self.project.path).await {
                Ok(start_result) => {
                    let state = start_result.state.clone();
                    let mut child = start_result.child;

                    // Spawn background log reader before storing child
                    super::logs::spawn_log_reader(
                        service.name.clone(),
                        &mut child,
                        Arc::clone(&self.states),
                        Arc::clone(&self.logs),
                    );

                    // Store child handle
                    let mut children = self.children.lock().await;
                    children.insert(service.name.clone(), child);

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
    /// If the service is already running, returns its current state without restarting.
    pub async fn start_one(&self, name: &str) -> Result<ServiceState> {
        let service = self
            .project
            .services
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| VoidStackError::ServiceNotFound {
                project: self.project.name.clone(),
                service: name.to_string(),
            })?;

        // Guard: don't start if already running
        if self.is_service_running(name).await {
            info!(service = %name, "Already running — skipping start");
            let states = self.states.lock().await;
            return Ok(states.get(name).cloned().unwrap_or_else(|| ServiceState::new(name.to_string())));
        }

        let runner = runner::runner_for(service.target);
        let start_result = runner.start(service, &self.project.path).await?;
        let state = start_result.state.clone();
        let mut child = start_result.child;

        super::logs::spawn_log_reader(
            name.to_string(),
            &mut child,
            Arc::clone(&self.states),
            Arc::clone(&self.logs),
        );

        let mut children = self.children.lock().await;
        children.insert(name.to_string(), child);

        let mut states = self.states.lock().await;
        states.insert(name.to_string(), state.clone());
        Ok(state)
    }

    /// Stop all running services.
    pub async fn stop_all(&self) -> Result<()> {
        info!("Stopping all services");

        // Collect names+pids under lock, then release before async work
        let to_stop: Vec<(String, u32)> = {
            let states = self.states.lock().await;
            states.iter()
                .filter(|(_, s)| s.status == ServiceStatus::Running && s.pid.is_some())
                .map(|(name, s)| (name.clone(), s.pid.unwrap()))
                .collect()
        };

        for (name, pid) in &to_stop {
            let service = self.project.services.iter().find(|s| s.name == *name);
            if let Some(service) = service {
                let runner = runner::runner_for(service.target);
                if let Err(e) = runner.stop(service, *pid).await {
                    error!(service = %name, error = %e, "Failed to stop");
                }
            }
        }

        // Verify processes died and update state
        if !to_stop.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        for (name, pid) in &to_stop {
            let service = self.project.services.iter().find(|s| s.name == *name);
            if let Some(service) = service {
                let runner = runner::runner_for(service.target);
                let still_running = runner.is_running(*pid).await.unwrap_or(false);
                if still_running {
                    // Retry kill
                    let _ = runner.stop(service, *pid).await;
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
            }
            // Update state regardless
            let mut states = self.states.lock().await;
            if let Some(state) = states.get_mut(name) {
                state.status = ServiceStatus::Stopped;
                state.pid = None;
            }
        }

        // Remove child handles
        let mut children = self.children.lock().await;
        for (name, _) in &to_stop {
            children.remove(name);
        }

        Ok(())
    }

    /// Stop a single service by name.
    pub async fn stop_one(&self, name: &str) -> Result<()> {
        let pid = {
            let states = self.states.lock().await;
            let state = states.get(name).ok_or_else(|| VoidStackError::ServiceNotFound {
                project: self.project.name.clone(),
                service: name.to_string(),
            })?;
            state.pid
        };

        if let Some(pid) = pid {
            let service = self
                .project
                .services
                .iter()
                .find(|s| s.name == name)
                .ok_or_else(|| VoidStackError::ServiceNotFound {
                    project: self.project.name.clone(),
                    service: name.to_string(),
                })?;

            let runner = runner::runner_for(service.target);
            runner.stop(service, pid).await?;

            // Wait and verify the process died
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let still_running = runner.is_running(pid).await.unwrap_or(false);
            if still_running {
                // Retry kill
                let _ = runner.stop(service, pid).await;
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }

            // Update state immediately
            let mut states = self.states.lock().await;
            if let Some(state) = states.get_mut(name) {
                state.status = ServiceStatus::Stopped;
                state.pid = None;
            }

            // Remove child handle
            let mut children = self.children.lock().await;
            children.remove(name);
        }

        Ok(())
    }
}
