use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use regex::Regex;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::backend::ServiceBackend;
use crate::error::{DevLaunchError, Result};
use crate::hooks;
use crate::model::{Project, ServiceState, ServiceStatus};
use crate::runner;

/// Max log lines kept per service.
const MAX_LOG_LINES: usize = 5000;

/// Manages the lifecycle of all services in a project.
pub struct ProcessManager {
    project: Project,
    states: Arc<Mutex<HashMap<String, ServiceState>>>,
    children: Mutex<HashMap<String, Child>>,
    logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
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

    /// Spawn a background task that reads lines from a child's stdout/stderr,
    /// stores them in logs, detects URLs, and updates last_log_line.
    fn spawn_log_reader(
        &self,
        service_name: String,
        child: &mut Child,
    ) {
        let states = Arc::clone(&self.states);
        let logs = Arc::clone(&self.logs);

        // Take stdout and stderr from child
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let name = service_name.clone();
        if let Some(stdout) = stdout {
            let states = Arc::clone(&states);
            let logs = Arc::clone(&logs);
            let name = name.clone();
            tokio::spawn(async move {
                info!(service = %name, "Log reader started (stdout)");
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    process_log_line(&name, &line, &states, &logs).await;
                }
                info!(service = %name, "Log reader ended (stdout)");
            });
        }

        if let Some(stderr) = stderr {
            let name2 = name.clone();
            tokio::spawn(async move {
                info!(service = %name2, "Log reader started (stderr)");
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    process_log_line(&name2, &line, &states, &logs).await;
                }
                info!(service = %name2, "Log reader ended (stderr)");
            });
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
                Ok(start_result) => {
                    let state = start_result.state.clone();
                    let mut child = start_result.child;

                    // Spawn background log reader before storing child
                    self.spawn_log_reader(service.name.clone(), &mut child);

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
        let start_result = runner.start(service, &self.project.path).await?;
        let state = start_result.state.clone();
        let mut child = start_result.child;

        self.spawn_log_reader(name.to_string(), &mut child);

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
            let state = states.get(name).ok_or_else(|| DevLaunchError::ServiceNotFound {
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
                .ok_or_else(|| DevLaunchError::ServiceNotFound {
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

/// Process a single log line: store it, detect URLs, update state.
async fn process_log_line(
    service_name: &str,
    line: &str,
    states: &Arc<Mutex<HashMap<String, ServiceState>>>,
    logs: &Arc<Mutex<HashMap<String, Vec<String>>>>,
) {
    debug!(service = %service_name, line = %line, "Captured log line");

    // Store in log buffer
    {
        let mut logs = logs.lock().await;
        if let Some(buf) = logs.get_mut(service_name) {
            buf.push(line.to_string());
            // Trim if too many lines
            if buf.len() > MAX_LOG_LINES {
                let drain = buf.len() - MAX_LOG_LINES;
                buf.drain(..drain);
            }
        }
    }

    // Update last_log_line and detect URLs
    {
        let mut states = states.lock().await;
        if let Some(state) = states.get_mut(service_name) {
            state.last_log_line = Some(line.to_string());

            // Detect URL if not yet found
            if state.url.is_none() {
                if let Some(url) = detect_url(line) {
                    info!(service = %service_name, url = %url, "Detected service URL");
                    state.url = Some(url);
                }
            }
        }
    }
}

/// Strip ANSI escape sequences (color codes, cursor movement, etc.)
fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
    re.replace_all(s, "").to_string()
}

/// Detect URLs like http://localhost:3000 or http://127.0.0.1:8000 from a log line.
fn detect_url(line: &str) -> Option<String> {
    // Strip ANSI codes first (Vite, Next.js, etc. colorize URLs)
    let clean = strip_ansi(line);

    // Common patterns output by dev servers
    let re = Regex::new(
        r#"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|::1)(?::\d+)(?:/[^\s\])\}>"']*)?"#
    ).ok()?;

    re.find(&clean).map(|m| {
        let url = m.as_str().to_string();
        // Normalize 0.0.0.0 to localhost for browser use
        url.replace("0.0.0.0", "localhost")
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_url_localhost() {
        assert_eq!(
            detect_url("Server running at http://localhost:3000"),
            Some("http://localhost:3000".to_string())
        );
    }

    #[test]
    fn test_detect_url_127() {
        assert_eq!(
            detect_url("Listening on http://127.0.0.1:8000/api"),
            Some("http://127.0.0.1:8000/api".to_string())
        );
    }

    #[test]
    fn test_detect_url_0000() {
        assert_eq!(
            detect_url("  ➜  Local:   http://0.0.0.0:5173/"),
            Some("http://localhost:5173/".to_string())
        );
    }

    #[test]
    fn test_detect_url_none() {
        assert_eq!(detect_url("Starting compilation..."), None);
    }

    #[test]
    fn test_detect_url_https() {
        assert_eq!(
            detect_url("Ready on https://localhost:3000"),
            Some("https://localhost:3000".to_string())
        );
    }

    #[test]
    fn test_detect_url_ansi_colored() {
        // Vite wraps URLs in ANSI color codes
        assert_eq!(
            detect_url("  ➜  Local:   \x1b[36mhttp://localhost:5173/\x1b[0m"),
            Some("http://localhost:5173/".to_string())
        );
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(
            strip_ansi("\x1b[36mhello\x1b[0m world"),
            "hello world"
        );
        assert_eq!(strip_ansi("no codes here"), "no codes here");
    }
}
