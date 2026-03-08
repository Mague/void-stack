use std::collections::HashMap;
use std::time::Instant;

use devlaunch_core::backend::ServiceBackend;
use devlaunch_core::model::{ServiceState, ServiceStatus, Target};

/// Which panel/mode the user is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Services,
    Logs,
}

/// Application state for the TUI dashboard.
pub struct App {
    pub backend: Box<dyn ServiceBackend>,
    pub project_name: String,
    pub service_names: Vec<String>,
    pub states: Vec<ServiceState>,
    pub selected: usize,
    pub focus: FocusPanel,
    pub show_help: bool,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub logs: HashMap<String, Vec<String>>,
    pub log_scroll: usize,
    pub started_time: Instant,
    pub daemon_mode: bool,
    pub service_targets: HashMap<String, Target>,
}

impl App {
    pub fn new(
        backend: Box<dyn ServiceBackend>,
        project_name: String,
        service_names: Vec<String>,
        service_targets: HashMap<String, Target>,
        daemon_mode: bool,
    ) -> Self {
        let states: Vec<ServiceState> = service_names
            .iter()
            .map(|n| ServiceState::new(n.clone()))
            .collect();
        let logs: HashMap<String, Vec<String>> = service_names
            .iter()
            .map(|n| (n.clone(), Vec::new()))
            .collect();

        Self {
            backend,
            project_name,
            service_names,
            states,
            selected: 0,
            focus: FocusPanel::Services,
            show_help: false,
            should_quit: false,
            status_message: None,
            logs,
            log_scroll: 0,
            started_time: Instant::now(),
            daemon_mode,
            service_targets,
        }
    }

    /// Sync internal states vec from the backend.
    pub async fn refresh_states(&mut self) {
        if let Err(e) = self.backend.refresh_status().await {
            self.status_message = Some(format!("Refresh error: {}", e));
        }
        match self.backend.get_states().await {
            Ok(mgr_states) => {
                self.states = self
                    .service_names
                    .iter()
                    .map(|name| {
                        mgr_states
                            .iter()
                            .find(|s| s.service_name == *name)
                            .cloned()
                            .unwrap_or_else(|| ServiceState::new(name.clone()))
                    })
                    .collect();

                // Fetch full logs from backend for each service
                for name in &self.service_names {
                    if let Ok(backend_logs) = self.backend.get_logs(name).await {
                        if let Some(buf) = self.logs.get_mut(name) {
                            // Only append new lines
                            let current_len = buf.len();
                            if backend_logs.len() > current_len {
                                buf.extend_from_slice(&backend_logs[current_len..]);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Get states error: {}", e));
            }
        }
    }

    pub fn selected_service_name(&self) -> Option<&str> {
        self.service_names.get(self.selected).map(|s| s.as_str())
    }

    pub fn move_up(&mut self) {
        if self.focus == FocusPanel::Services {
            if self.selected > 0 {
                self.selected -= 1;
            }
            self.log_scroll = 0;
        } else {
            if self.log_scroll > 0 {
                self.log_scroll -= 1;
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.focus == FocusPanel::Services {
            if !self.service_names.is_empty() && self.selected < self.service_names.len() - 1 {
                self.selected += 1;
            }
            self.log_scroll = 0;
        } else {
            self.log_scroll += 1;
        }
    }

    pub async fn start_all(&mut self) {
        self.status_message = Some("Starting all services...".to_string());
        match self.backend.start_all().await {
            Ok(results) => {
                let ok_count = results
                    .iter()
                    .filter(|s| s.status == ServiceStatus::Running)
                    .count();
                let fail_count = results.len() - ok_count;
                self.status_message = Some(format!(
                    "Started {ok_count} service(s), {fail_count} failed"
                ));
                for r in &results {
                    self.push_log(
                        &r.service_name,
                        format!("[devlaunch] {} -> {}", r.service_name, r.status),
                    );
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Start all failed: {e}"));
            }
        }
        self.refresh_states().await;
    }

    pub async fn start_selected(&mut self) {
        let name = match self.selected_service_name() {
            Some(n) => n.to_string(),
            None => return,
        };
        self.status_message = Some(format!("Starting {name}..."));
        match self.backend.start_one(&name).await {
            Ok(state) => {
                self.push_log(
                    &name,
                    format!("[devlaunch] {} -> {}", name, state.status),
                );
                self.status_message = Some(format!("{name} started (PID {:?})", state.pid));
            }
            Err(e) => {
                self.push_log(&name, format!("[devlaunch] ERROR: {e}"));
                self.status_message = Some(format!("Failed to start {name}: {e}"));
            }
        }
        self.refresh_states().await;
    }

    pub async fn stop_selected(&mut self) {
        let name = match self.selected_service_name() {
            Some(n) => n.to_string(),
            None => return,
        };
        self.status_message = Some(format!("Stopping {name}..."));
        match self.backend.stop_one(&name).await {
            Ok(()) => {
                self.push_log(&name, format!("[devlaunch] {name} stopped"));
                self.status_message = Some(format!("{name} stopped"));
            }
            Err(e) => {
                self.push_log(&name, format!("[devlaunch] stop error: {e}"));
                self.status_message = Some(format!("Failed to stop {name}: {e}"));
            }
        }
        self.refresh_states().await;
    }

    pub async fn stop_all(&mut self) {
        self.status_message = Some("Stopping all services...".to_string());
        if let Err(e) = self.backend.stop_all().await {
            self.status_message = Some(format!("Stop all error: {e}"));
        } else {
            self.status_message = Some("All services stopped".to_string());
        }
        self.refresh_states().await;
    }

    fn push_log(&mut self, service_name: &str, line: String) {
        if let Some(buf) = self.logs.get_mut(service_name) {
            buf.push(line);
        }
    }

    /// Get logs for the currently selected service.
    pub fn selected_logs(&self) -> &[String] {
        self.selected_service_name()
            .and_then(|name| self.logs.get(name))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn running_count(&self) -> usize {
        self.states
            .iter()
            .filter(|s| s.status == ServiceStatus::Running)
            .count()
    }

    pub fn total_count(&self) -> usize {
        self.service_names.len()
    }
}
