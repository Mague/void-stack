use std::collections::HashMap;
use std::time::Instant;

use devlaunch_core::manager::ProcessManager;
use devlaunch_core::model::{Project, ServiceState, ServiceStatus};

/// Which panel/mode the user is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Services,
    Logs,
}

/// Application state for the TUI dashboard.
pub struct App {
    pub manager: ProcessManager,
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
}

impl App {
    pub fn new(project: Project) -> Self {
        let service_names: Vec<String> = project.services.iter().map(|s| s.name.clone()).collect();
        let states: Vec<ServiceState> = service_names
            .iter()
            .map(|n| ServiceState::new(n.clone()))
            .collect();
        let logs: HashMap<String, Vec<String>> = service_names
            .iter()
            .map(|n| (n.clone(), Vec::new()))
            .collect();
        let project_name = project.name.clone();
        let manager = ProcessManager::new(project);

        Self {
            manager,
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
        }
    }

    /// Sync internal states vec from the process manager.
    pub async fn refresh_states(&mut self) {
        if let Err(e) = self.manager.refresh_status().await {
            self.status_message = Some(format!("Refresh error: {}", e));
        }
        let mgr_states = self.manager.get_states().await;
        // Rebuild states vec in the same order as service_names
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

        // Capture last_log_line into per-service log buffer
        for state in &self.states {
            if let Some(line) = &state.last_log_line {
                if !line.is_empty() {
                    if let Some(buf) = self.logs.get_mut(&state.service_name) {
                        if buf.last().map(|l| l.as_str()) != Some(line.as_str()) {
                            buf.push(line.clone());
                        }
                    }
                }
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
        match self.manager.start_all().await {
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
        match self.manager.start_one(&name).await {
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
        match self.manager.stop_one(&name).await {
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
        if let Err(e) = self.manager.stop_all().await {
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
