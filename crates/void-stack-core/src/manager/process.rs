use std::sync::Arc;
use tracing::{error, info};

use super::ProcessManager;
use crate::error::{Result, VoidStackError};
use crate::hooks;
use crate::model::{ServiceState, ServiceStatus};
use crate::runner;

/// Collect (name, pid, target) tuples for all running services.
/// This is the pure logic extracted from stop_all for testability.
pub(crate) async fn collect_running_pids(
    mgr: &ProcessManager,
) -> Vec<(String, u32, crate::model::Target)> {
    let states = mgr.states.lock().await;
    states
        .iter()
        .filter(|(_, s)| s.status == ServiceStatus::Running && s.pid.is_some())
        .filter_map(|(name, s)| {
            let service = mgr.project.services.iter().find(|svc| svc.name == *name)?;
            Some((name.clone(), s.pid.unwrap(), service.target))
        })
        .collect()
}

impl ProcessManager {
    /// Check if a service is currently running (has a live PID).
    pub(crate) async fn is_service_running(&self, name: &str) -> bool {
        let states = self.states.lock().await;
        if let Some(state) = states.get(name)
            && state.status == ServiceStatus::Running
            && let Some(pid) = state.pid
        {
            // Verify the PID is actually alive
            let service = self.project.services.iter().find(|s| s.name == name);
            if let Some(service) = service {
                let runner = runner::runner_for(service.target);
                return runner.is_running(pid).await.unwrap_or(false);
            }
        }
        false
    }

    /// Start all enabled services in parallel.
    pub async fn start_all(&self) -> Result<Vec<ServiceState>> {
        let enabled: Vec<_> = self.project.services.iter().filter(|s| s.enabled).collect();

        info!(count = enabled.len(), "Starting all enabled services");

        // Run pre-launch hooks per service working_dir (or project root).
        // Uses configured hooks or sensible defaults (venv + install_deps).
        let hook_config = self
            .project
            .hooks
            .clone()
            .unwrap_or(crate::model::HookConfig {
                venv: true,
                install_deps: true,
                build: false,
                custom: vec![],
            });
        // Collect unique directories to run hooks in
        let mut hook_dirs: Vec<String> = vec![self.project.path.clone()];
        for svc in &enabled {
            if let Some(ref wd) = svc.working_dir
                && !hook_dirs.contains(wd)
            {
                hook_dirs.push(wd.clone());
            }
        }
        for dir in &hook_dirs {
            let dir_path = std::path::Path::new(dir);
            let dir_type = self
                .project
                .project_type
                .unwrap_or_else(|| crate::config::detect_project_type(dir_path));
            if let Err(e) = hooks::run_pre_launch(&hook_config, dir, Some(dir_type)).await {
                // Log but don't fail — the service might still work
                tracing::warn!(dir = %dir, error = %e, "Pre-launch hook failed");
            }
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
                    let child = start_result.child;

                    // Spawn background log reader + exit watcher (takes ownership of child)
                    super::logs::spawn_log_reader(
                        service.name.clone(),
                        child,
                        Arc::clone(&self.states),
                        Arc::clone(&self.logs),
                    );

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
            return Ok(states
                .get(name)
                .cloned()
                .unwrap_or_else(|| ServiceState::new(name.to_string())));
        }

        let runner = runner::runner_for(service.target);
        let start_result = runner.start(service, &self.project.path).await?;
        let state = start_result.state.clone();
        let child = start_result.child;

        super::logs::spawn_log_reader(
            name.to_string(),
            child,
            Arc::clone(&self.states),
            Arc::clone(&self.logs),
        );

        let mut states = self.states.lock().await;
        states.insert(name.to_string(), state.clone());
        Ok(state)
    }

    /// Stop all running services in parallel.
    pub async fn stop_all(&self) -> Result<()> {
        info!("Stopping all services");

        let to_stop = collect_running_pids(self).await;

        if to_stop.is_empty() {
            return Ok(());
        }

        // Send all kill signals in parallel
        let stop_handles: Vec<_> = to_stop
            .iter()
            .map(|(name, pid, target)| {
                let name = name.clone();
                let pid = *pid;
                let target = *target;
                let services = self.project.services.clone();
                tokio::spawn(async move {
                    let runner = runner::runner_for(target);
                    if let Some(service) = services.iter().find(|s| s.name == name)
                        && let Err(e) = runner.stop(service, pid).await
                    {
                        error!(service = %name, error = %e, "Failed to stop");
                    }
                })
            })
            .collect();

        for handle in stop_handles {
            let _ = handle.await;
        }

        // One global sleep for processes to die
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // Verify all processes died in parallel
        let check_handles: Vec<_> = to_stop
            .iter()
            .map(|(name, pid, target)| {
                let name = name.clone();
                let pid = *pid;
                let target = *target;
                let services = self.project.services.clone();
                tokio::spawn(async move {
                    let runner = runner::runner_for(target);
                    let still_running = runner.is_running(pid).await.unwrap_or(false);
                    if still_running && let Some(service) = services.iter().find(|s| s.name == name)
                    {
                        let _ = runner.stop(service, pid).await;
                    }
                })
            })
            .collect();

        for handle in check_handles {
            let _ = handle.await;
        }

        // Update all states and remove child handles
        {
            let mut states = self.states.lock().await;
            for (name, _, _) in &to_stop {
                if let Some(state) = states.get_mut(name) {
                    state.status = ServiceStatus::Stopped;
                    state.pid = None;
                }
            }
        }
        Ok(())
    }

    /// Stop a single service by name.
    pub async fn stop_one(&self, name: &str) -> Result<()> {
        let pid = {
            let states = self.states.lock().await;
            let state = states
                .get(name)
                .ok_or_else(|| VoidStackError::ServiceNotFound {
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
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, Service, Target};

    fn make_echo_project() -> Project {
        Project {
            name: "test-echo".into(),
            path: std::env::temp_dir().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![Service {
                name: "echo-svc".into(),
                command: "cmd /c echo hello".into(),
                target: Target::native(),
                working_dir: Some(std::env::temp_dir().to_string_lossy().to_string()),
                enabled: true,
                env_vars: vec![],
                depends_on: vec![],
                docker: None,
            }],
            hooks: None,
        }
    }

    #[tokio::test]
    async fn test_process_manager_new_initializes_states() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);

        let states = manager.states.lock().await;
        assert!(states.contains_key("echo-svc"));
        assert_eq!(states["echo-svc"].status, ServiceStatus::Stopped);
        assert_eq!(states["echo-svc"].pid, None);
    }

    #[tokio::test]
    async fn test_is_service_running_unknown_service() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);
        assert!(!manager.is_service_running("nonexistent").await);
    }

    #[tokio::test]
    async fn test_is_service_running_stopped_service() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);
        assert!(!manager.is_service_running("echo-svc").await);
    }

    #[tokio::test]
    async fn test_collect_running_pids_empty_when_all_stopped() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);
        let pids = collect_running_pids(&manager).await;
        assert!(pids.is_empty());
    }

    #[tokio::test]
    async fn test_collect_running_pids_finds_running_service() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);

        // Manually mark a service as running with a fake PID
        {
            let mut states = manager.states.lock().await;
            if let Some(state) = states.get_mut("echo-svc") {
                state.status = ServiceStatus::Running;
                state.pid = Some(99999);
            }
        }

        let pids = collect_running_pids(&manager).await;
        assert_eq!(pids.len(), 1);
        assert_eq!(pids[0].0, "echo-svc");
        assert_eq!(pids[0].1, 99999);
    }

    #[tokio::test]
    async fn test_stop_all_no_panic_when_empty() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);
        // stop_all on already-stopped services should succeed
        let result = manager.stop_all().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stop_one_unknown_service_returns_error() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);
        let result = manager.stop_one("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stop_one_stopped_service_is_noop() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);
        // echo-svc exists but has no PID — stop_one should be a no-op
        let result = manager.stop_one("echo-svc").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_all_with_echo_service() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);

        let states = manager.start_all().await.unwrap();
        assert_eq!(states.len(), 1);
        // echo terminates quickly so status may be Running or Failed
        // but it should not panic
    }

    #[tokio::test]
    async fn test_start_one_with_echo_service() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);

        let state = manager.start_one("echo-svc").await.unwrap();
        assert_eq!(state.service_name, "echo-svc");
    }

    #[tokio::test]
    async fn test_start_one_unknown_service_returns_error() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);
        let result = manager.start_one("nonexistent").await;
        assert!(result.is_err());
    }
}
