use std::collections::HashMap;
use std::time::Instant;

use void_stack_core::backend::ServiceBackend;
use void_stack_core::detector::DependencyStatus;
use void_stack_core::model::{ServiceState, ServiceStatus, Target};

/// Which panel the user is focused on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Projects,
    Services,
    Logs,
}

/// A project loaded in the TUI with its own backend.
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub backend: Box<dyn ServiceBackend>,
    pub service_names: Vec<String>,
    pub service_targets: HashMap<String, Target>,
    pub service_dirs: Vec<Option<String>>,
    pub states: Vec<ServiceState>,
    pub logs: HashMap<String, Vec<String>>,
    pub deps: Vec<DependencyStatus>,
    pub deps_checked: bool,
}

/// Application state for the multi-project TUI dashboard.
pub struct App {
    pub projects: Vec<ProjectEntry>,
    pub selected_project: usize,
    pub selected_service: usize,
    pub focus: FocusPanel,
    pub show_help: bool,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub log_scroll: usize,
    pub started_time: Instant,
}

impl App {
    pub fn new(projects: Vec<ProjectEntry>) -> Self {
        Self {
            projects,
            selected_project: 0,
            selected_service: 0,
            focus: FocusPanel::Projects,
            show_help: false,
            should_quit: false,
            status_message: None,
            log_scroll: 0,
            started_time: Instant::now(),
        }
    }

    /// Get the currently selected project, if any.
    pub fn current_project(&self) -> Option<&ProjectEntry> {
        self.projects.get(self.selected_project)
    }

    /// Currently selected service name within the active project.
    pub fn selected_service_name(&self) -> Option<&str> {
        self.current_project()
            .and_then(|p| p.service_names.get(self.selected_service).map(|s| s.as_str()))
    }

    /// Logs for the currently selected service.
    pub fn selected_logs(&self) -> &[String] {
        self.selected_service_name()
            .and_then(|name| {
                self.current_project()
                    .and_then(|p| p.logs.get(name))
            })
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Total running services across all projects.
    pub fn total_running(&self) -> usize {
        self.projects
            .iter()
            .flat_map(|p| &p.states)
            .filter(|s| s.status == ServiceStatus::Running)
            .count()
    }

    /// Total services across all projects.
    pub fn total_services(&self) -> usize {
        self.projects.iter().map(|p| p.service_names.len()).sum()
    }

    /// Service count for the current project.
    pub fn current_total(&self) -> usize {
        self.current_project()
            .map(|p| p.service_names.len())
            .unwrap_or(0)
    }

    // ── Navigation ──────────────────────────────────────────

    pub fn move_up(&mut self) {
        match self.focus {
            FocusPanel::Projects => {
                if self.selected_project > 0 {
                    self.selected_project -= 1;
                    self.selected_service = 0;
                    self.log_scroll = 0;
                }
            }
            FocusPanel::Services => {
                if self.selected_service > 0 {
                    self.selected_service -= 1;
                    self.log_scroll = 0;
                }
            }
            FocusPanel::Logs => {
                if self.log_scroll > 0 {
                    self.log_scroll -= 1;
                }
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.focus {
            FocusPanel::Projects => {
                if !self.projects.is_empty() && self.selected_project < self.projects.len() - 1 {
                    self.selected_project += 1;
                    self.selected_service = 0;
                    self.log_scroll = 0;
                }
            }
            FocusPanel::Services => {
                let max = self.current_total();
                if max > 0 && self.selected_service < max - 1 {
                    self.selected_service += 1;
                    self.log_scroll = 0;
                }
            }
            FocusPanel::Logs => {
                self.log_scroll += 1;
            }
        }
    }

    pub fn next_panel(&mut self) {
        self.focus = match self.focus {
            FocusPanel::Projects => FocusPanel::Services,
            FocusPanel::Services => FocusPanel::Logs,
            FocusPanel::Logs => FocusPanel::Projects,
        };
    }

    pub fn prev_panel(&mut self) {
        self.focus = match self.focus {
            FocusPanel::Projects => FocusPanel::Logs,
            FocusPanel::Services => FocusPanel::Projects,
            FocusPanel::Logs => FocusPanel::Services,
        };
    }

    // ── Service actions ─────────────────────────────────────

    pub async fn refresh_current(&mut self) {
        let idx = self.selected_project;
        if let Some(project) = self.projects.get_mut(idx) {
            let _ = project.backend.refresh_status().await;
            if let Ok(mgr_states) = project.backend.get_states().await {
                project.states = project
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

                // Fetch logs
                for name in &project.service_names {
                    if let Ok(backend_logs) = project.backend.get_logs(name).await {
                        if let Some(buf) = project.logs.get_mut(name) {
                            let current_len = buf.len();
                            if backend_logs.len() > current_len {
                                buf.extend_from_slice(&backend_logs[current_len..]);
                            }
                        }
                    }
                }
            }
        }
    }

    pub async fn refresh_all(&mut self) {
        for project in &mut self.projects {
            let _ = project.backend.refresh_status().await;
            if let Ok(mgr_states) = project.backend.get_states().await {
                project.states = project
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

                for name in &project.service_names {
                    if let Ok(backend_logs) = project.backend.get_logs(name).await {
                        if let Some(buf) = project.logs.get_mut(name) {
                            let current_len = buf.len();
                            if backend_logs.len() > current_len {
                                buf.extend_from_slice(&backend_logs[current_len..]);
                            }
                        }
                    }
                }
            }
        }
    }

    pub async fn start_all(&mut self) {
        let idx = self.selected_project;
        if let Some(project) = self.projects.get_mut(idx) {
            self.status_message = Some(format!("Starting all {} services...", project.name));
            match project.backend.start_all().await {
                Ok(results) => {
                    let ok = results.iter().filter(|s| s.status == ServiceStatus::Running).count();
                    let fail = results.len() - ok;
                    self.status_message = Some(format!(
                        "{}: {} started, {} failed", project.name, ok, fail
                    ));
                    for r in &results {
                        if let Some(buf) = project.logs.get_mut(&r.service_name) {
                            buf.push(format!("[void-stack] {} -> {}", r.service_name, r.status));
                        }
                    }
                }
                Err(e) => {
                    self.status_message = Some(format!("Start all failed: {e}"));
                }
            }
        }
        self.refresh_current().await;
    }

    pub async fn start_selected(&mut self) {
        let name = match self.selected_service_name() {
            Some(n) => n.to_string(),
            None => return,
        };
        let idx = self.selected_project;
        if let Some(project) = self.projects.get_mut(idx) {
            self.status_message = Some(format!("Starting {}...", name));
            match project.backend.start_one(&name).await {
                Ok(state) => {
                    if let Some(buf) = project.logs.get_mut(&name) {
                        buf.push(format!("[void-stack] {} -> {}", name, state.status));
                    }
                    self.status_message = Some(format!("{} started (PID {:?})", name, state.pid));
                }
                Err(e) => {
                    if let Some(buf) = project.logs.get_mut(&name) {
                        buf.push(format!("[void-stack] ERROR: {e}"));
                    }
                    self.status_message = Some(format!("Failed to start {name}: {e}"));
                }
            }
        }
        self.refresh_current().await;
    }

    pub async fn stop_selected(&mut self) {
        let name = match self.selected_service_name() {
            Some(n) => n.to_string(),
            None => return,
        };
        let idx = self.selected_project;
        if let Some(project) = self.projects.get_mut(idx) {
            self.status_message = Some(format!("Stopping {}...", name));
            match project.backend.stop_one(&name).await {
                Ok(()) => {
                    if let Some(buf) = project.logs.get_mut(&name) {
                        buf.push(format!("[void-stack] {} stopped", name));
                    }
                    self.status_message = Some(format!("{name} stopped"));
                }
                Err(e) => {
                    self.status_message = Some(format!("Failed to stop {name}: {e}"));
                }
            }
        }
        self.refresh_current().await;
    }

    pub async fn stop_all(&mut self) {
        let idx = self.selected_project;
        if let Some(project) = self.projects.get_mut(idx) {
            self.status_message = Some(format!("Stopping all {} services...", project.name));
            if let Err(e) = project.backend.stop_all().await {
                self.status_message = Some(format!("Stop all error: {e}"));
            } else {
                self.status_message = Some(format!("{}: all services stopped", project.name));
            }
        }
        self.refresh_current().await;
    }

    /// Check dependencies for the current project.
    pub async fn check_deps(&mut self) {
        let idx = self.selected_project;
        if let Some(project) = self.projects.get_mut(idx) {
            self.status_message = Some(format!("Checking dependencies for {}...", project.name));

            let stripped = void_stack_core::runner::local::strip_win_prefix(&project.path);
            let mut dirs: Vec<std::path::PathBuf> = vec![std::path::PathBuf::from(&stripped)];
            for dir in &project.service_dirs {
                if let Some(d) = dir {
                    let s = void_stack_core::runner::local::strip_win_prefix(d);
                    let p = std::path::PathBuf::from(&s);
                    if !dirs.contains(&p) {
                        dirs.push(p);
                    }
                }
            }

            let mut seen = std::collections::HashSet::new();
            let mut results = Vec::new();
            for dir in &dirs {
                for dep in void_stack_core::detector::check_project(&dir).await {
                    if seen.insert(format!("{:?}", dep.dep_type)) {
                        results.push(dep);
                    }
                }
            }

            let ok = results.iter().filter(|d| matches!(d.status, void_stack_core::detector::CheckStatus::Ok)).count();
            let total = results.len();
            project.deps = results;
            project.deps_checked = true;
            self.status_message = Some(format!("{}: {}/{} deps ready", project.name, ok, total));
        }
    }

    /// Stop all services across ALL projects (used on quit).
    pub async fn stop_everything(&mut self) {
        for project in &mut self.projects {
            let _ = project.backend.stop_all().await;
        }
    }
}
