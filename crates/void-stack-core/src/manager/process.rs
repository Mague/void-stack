use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use super::ProcessManager;
use crate::error::{Result, VoidStackError};
use crate::hooks;
use crate::model::{Service, ServiceState, ServiceStatus};
use crate::runner;

/// How long to wait for a process to exit after the termination signal
/// before escalating with a second stop attempt.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Send the termination signal and wait for the process to actually exit.
///
/// When the manager owns the child (we spawned it this session), `exit_rx`
/// is the watch channel fed by the `child.wait()` task — we await it with a
/// timeout and escalate by re-sending the stop on expiry. Only when the
/// handle is unavailable (e.g. a process adopted after a daemon restart) do
/// we fall back to bounded PID polling, which is inherently racy under PID
/// reuse.
pub(crate) async fn stop_service_process(
    service: &Service,
    pid: u32,
    exit_rx: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<()> {
    let runner = runner::runner_for(service.target);
    runner.stop(service, pid).await?;

    match exit_rx {
        Some(mut rx) => {
            let exited = tokio::time::timeout(STOP_GRACE, rx.wait_for(|exited| *exited))
                .await
                .is_ok();
            if !exited {
                warn!(service = %service.name, pid = pid, "Process did not exit within grace period — escalating");
                let _ = runner.stop(service, pid).await;
                let _ = tokio::time::timeout(STOP_GRACE, rx.wait_for(|exited| *exited)).await;
            }
        }
        None => {
            if !wait_for_pid_exit(runner.as_ref(), pid, STOP_GRACE).await {
                warn!(service = %service.name, pid = pid, "Process did not exit within grace period (no handle) — escalating");
                let _ = runner.stop(service, pid).await;
                wait_for_pid_exit(runner.as_ref(), pid, STOP_GRACE).await;
            }
        }
    }

    Ok(())
}

/// Poll until the PID is gone or the grace period expires.
/// Returns `true` if the process exited.
async fn wait_for_pid_exit(runner: &dyn runner::Runner, pid: u32, grace: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        if !runner.is_running(pid).await.unwrap_or(false) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Collect (name, pid, target) tuples for all running services.
/// This is the pure logic extracted from stop_all for testability.
pub(crate) async fn collect_running_pids(
    mgr: &ProcessManager,
) -> Vec<(String, u32, crate::model::Target)> {
    let states = mgr.states.lock().await;
    states
        .iter()
        .filter(|(_, s)| s.status == ServiceStatus::Running)
        .filter_map(|(name, s)| {
            let pid = s.pid?;
            let service = mgr.project.services.iter().find(|svc| svc.name == *name)?;
            Some((name.clone(), pid, service.target))
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
                    let exit_rx = super::logs::spawn_log_reader(
                        service.name.clone(),
                        child,
                        Arc::clone(&self.states),
                        Arc::clone(&self.logs),
                    );
                    self.exit_watchers
                        .lock()
                        .await
                        .insert(service.name.clone(), exit_rx);

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

        let exit_rx = super::logs::spawn_log_reader(
            name.to_string(),
            child,
            Arc::clone(&self.states),
            Arc::clone(&self.logs),
        );
        self.exit_watchers
            .lock()
            .await
            .insert(name.to_string(), exit_rx);

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

        // Grab the exit receivers up front so the spawned tasks don't need
        // the watchers lock.
        let receivers: std::collections::HashMap<String, tokio::sync::watch::Receiver<bool>> = {
            let watchers = self.exit_watchers.lock().await;
            to_stop
                .iter()
                .filter_map(|(name, _, _)| watchers.get(name).map(|rx| (name.clone(), rx.clone())))
                .collect()
        };

        // Stop every service in parallel, each waiting for its own exit.
        let stop_handles: Vec<_> = to_stop
            .iter()
            .map(|(name, pid, _)| {
                let name = name.clone();
                let pid = *pid;
                let service = self
                    .project
                    .services
                    .iter()
                    .find(|s| s.name == name)
                    .cloned();
                let exit_rx = receivers.get(&name).cloned();
                tokio::spawn(async move {
                    if let Some(service) = service
                        && let Err(e) = stop_service_process(&service, pid, exit_rx).await
                    {
                        error!(service = %name, error = %e, "Failed to stop");
                    }
                })
            })
            .collect();

        for handle in stop_handles {
            let _ = handle.await;
        }

        // Update all states and drop the exit watchers
        {
            let mut states = self.states.lock().await;
            for (name, _, _) in &to_stop {
                if let Some(state) = states.get_mut(name) {
                    state.status = ServiceStatus::Stopped;
                    state.pid = None;
                }
            }
        }
        {
            let mut watchers = self.exit_watchers.lock().await;
            for (name, _, _) in &to_stop {
                watchers.remove(name);
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

            let exit_rx = { self.exit_watchers.lock().await.get(name).cloned() };
            stop_service_process(service, pid, exit_rx).await?;

            // Update state and drop the exit watcher
            {
                let mut states = self.states.lock().await;
                if let Some(state) = states.get_mut(name) {
                    state.status = ServiceStatus::Stopped;
                    state.pid = None;
                }
            }
            self.exit_watchers.lock().await.remove(name);
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

    #[cfg(unix)]
    fn make_sleep_project() -> Project {
        Project {
            name: "test-sleep".into(),
            path: std::env::temp_dir().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![Service {
                name: "sleep-svc".into(),
                command: "sleep 30".into(),
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

    /// stop_one must wait on the child handle (exit watcher), not sleep a
    /// fixed interval: the process must be dead when stop_one returns and
    /// the call must finish well under the escalation grace period.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_stop_one_waits_for_child_exit() {
        let project = make_sleep_project();
        let manager = ProcessManager::new(project);

        let state = manager.start_one("sleep-svc").await.unwrap();
        let pid = state.pid.unwrap();
        assert!(manager.exit_watchers.lock().await.contains_key("sleep-svc"));

        let started = std::time::Instant::now();
        manager.stop_one("sleep-svc").await.unwrap();
        let elapsed = started.elapsed();

        assert!(
            !crate::process_util::is_pid_alive_sync(pid),
            "process must be dead when stop_one returns"
        );
        assert!(
            elapsed < STOP_GRACE,
            "stop_one should return on exit notification, not escalation timeout (took {:?})",
            elapsed
        );

        let states = manager.states.lock().await;
        assert_eq!(states["sleep-svc"].status, ServiceStatus::Stopped);
        assert_eq!(states["sleep-svc"].pid, None);
        drop(states);
        assert!(!manager.exit_watchers.lock().await.contains_key("sleep-svc"));
    }

    /// When the manager has no child handle (e.g. a PID adopted after a
    /// daemon restart) and the PID is already dead, stop_one must fall back
    /// to the PID check and return promptly without escalating.
    #[tokio::test]
    async fn test_stop_one_without_handle_falls_back_to_pid_check() {
        let project = make_echo_project();
        let manager = ProcessManager::new(project);

        // Dead/foreign PID, no exit watcher registered.
        {
            let mut states = manager.states.lock().await;
            if let Some(state) = states.get_mut("echo-svc") {
                state.status = ServiceStatus::Running;
                state.pid = Some(4_000_000);
            }
        }

        let started = std::time::Instant::now();
        manager.stop_one("echo-svc").await.unwrap();
        assert!(
            started.elapsed() < STOP_GRACE,
            "dead PID should be detected immediately by the fallback poll"
        );

        let states = manager.states.lock().await;
        assert_eq!(states["echo-svc"].status, ServiceStatus::Stopped);
        assert_eq!(states["echo-svc"].pid, None);
    }
}
