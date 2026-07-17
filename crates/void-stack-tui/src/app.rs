use std::collections::HashMap;
use std::time::Instant;

use void_stack_core::backend::ServiceBackend;
use void_stack_core::detector::DependencyStatus;
use void_stack_core::model::{ServiceState, ServiceStatus, Target};

use crate::i18n::Lang;

/// Which panel the user is focused on within the Services tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Projects,
    Services,
    Logs,
}

/// Top-level tabs in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTab {
    Services,
    Analysis,
    Security,
    Debt,
    Space,
    Stats,
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
    pub active_tab: AppTab,
    pub show_help: bool,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub log_scroll: usize,
    pub started_time: Instant,
    pub lang: Lang,

    // Cached analysis data (per project — reset on project switch)
    pub analysis_result: Option<void_stack_core::analyzer::AnalysisResult>,
    pub analysis_loading: bool,
    pub audit_result: Option<void_stack_core::audit::AuditResult>,
    pub audit_loading: bool,
    pub debt_items: Option<Vec<void_stack_core::analyzer::explicit_debt::ExplicitDebtItem>>,
    pub debt_loading: bool,
    pub space_entries: Option<Vec<void_stack_core::space::SpaceEntry>>,
    pub space_loading: bool,

    // Log filtering
    pub log_filter_active: bool,
    pub log_filter_savings: Option<f32>,

    // Stats
    pub stats_report: Option<void_stack_core::stats::StatsReport>,
    pub stats_loading: bool,

    // Semantic search (vector feature)
    pub search_input: String,
    pub search_active: bool,
    #[cfg(feature = "vector")]
    pub search_results: Option<Vec<void_stack_core::vector_index::SearchResult>>,
    pub search_loading: bool,
    pub index_exists: bool,
    pub indexing: bool,

    // AI suggestions
    pub suggest_output: Option<String>,
    pub suggesting: bool,
}

impl App {
    pub fn new(projects: Vec<ProjectEntry>) -> Self {
        Self {
            projects,
            selected_project: 0,
            selected_service: 0,
            focus: FocusPanel::Projects,
            active_tab: AppTab::Services,
            show_help: false,
            should_quit: false,
            status_message: None,
            log_scroll: 0,
            started_time: Instant::now(),
            lang: Lang::Es,
            analysis_result: None,
            analysis_loading: false,
            audit_result: None,
            audit_loading: false,
            debt_items: None,
            debt_loading: false,
            space_entries: None,
            space_loading: false,
            log_filter_active: false,
            log_filter_savings: None,
            stats_report: None,
            stats_loading: false,
            search_input: String::new(),
            search_active: false,
            #[cfg(feature = "vector")]
            search_results: None,
            search_loading: false,
            index_exists: false,
            indexing: false,
            suggest_output: None,
            suggesting: false,
        }
    }

    /// Reset cached analysis data (called on project switch).
    pub fn reset_tab_data(&mut self) {
        self.analysis_result = None;
        self.analysis_loading = false;
        self.audit_result = None;
        self.audit_loading = false;
        self.debt_items = None;
        self.debt_loading = false;
        self.space_entries = None;
        self.space_loading = false;
        self.stats_report = None;
        self.stats_loading = false;
        self.search_input.clear();
        self.search_active = false;
        #[cfg(feature = "vector")]
        {
            self.search_results = None;
        }
        self.search_loading = false;
        self.index_exists = false;
        self.indexing = false;
        self.suggest_output = None;
        self.suggesting = false;
    }

    /// Get the currently selected project, if any.
    pub fn current_project(&self) -> Option<&ProjectEntry> {
        self.projects.get(self.selected_project)
    }

    /// Currently selected service name within the active project.
    pub fn selected_service_name(&self) -> Option<&str> {
        self.current_project().and_then(|p| {
            p.service_names
                .get(self.selected_service)
                .map(|s| s.as_str())
        })
    }

    /// Logs for the currently selected service.
    pub fn selected_logs(&self) -> &[String] {
        self.selected_service_name()
            .and_then(|name| self.current_project().and_then(|p| p.logs.get(name)))
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
                    self.reset_tab_data();
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
                    self.reset_tab_data();
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
                    if let Ok(backend_logs) = project.backend.get_logs(name).await
                        && let Some(buf) = project.logs.get_mut(name)
                    {
                        let current_len = buf.len();
                        if backend_logs.len() > current_len {
                            buf.extend_from_slice(&backend_logs[current_len..]);
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
                    if let Ok(backend_logs) = project.backend.get_logs(name).await
                        && let Some(buf) = project.logs.get_mut(name)
                    {
                        let current_len = buf.len();
                        if backend_logs.len() > current_len {
                            buf.extend_from_slice(&backend_logs[current_len..]);
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
                    let ok = results
                        .iter()
                        .filter(|s| s.status == ServiceStatus::Running)
                        .count();
                    let fail = results.len() - ok;
                    self.status_message =
                        Some(format!("{}: {} started, {} failed", project.name, ok, fail));
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
            for d in project.service_dirs.iter().flatten() {
                let s = void_stack_core::runner::local::strip_win_prefix(d);
                let p = std::path::PathBuf::from(&s);
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }

            let mut seen = std::collections::HashSet::new();
            let mut results = Vec::new();
            for dir in &dirs {
                for dep in void_stack_core::detector::check_project(dir).await {
                    if seen.insert(format!("{:?}", dep.dep_type)) {
                        results.push(dep);
                    }
                }
            }

            let ok = results
                .iter()
                .filter(|d| matches!(d.status, void_stack_core::detector::CheckStatus::Ok))
                .count();
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

#[cfg(test)]
pub(crate) mod test_support {
    //! Shared test fixtures: an in-memory `ServiceBackend` plus builders for
    //! `ProjectEntry` / `App` that never spawn processes or hit a daemon.

    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;

    use void_stack_core::backend::ServiceBackend;
    use void_stack_core::error::Result as CoreResult;
    use void_stack_core::model::{ServiceState, ServiceStatus, Target};

    use super::{App, ProjectEntry};

    /// Boxed future type matching the `#[async_trait]` expansion used by
    /// `ServiceBackend`.
    type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

    /// Build a `ServiceState` with the given status (running states get a PID).
    pub fn state(name: &str, status: ServiceStatus) -> ServiceState {
        let mut s = ServiceState::new(name.to_string());
        s.status = status;
        if status == ServiceStatus::Running {
            s.pid = Some(1234);
            s.started_at = Some(chrono::Utc::now());
            s.url = Some("http://localhost:3000".to_string());
        }
        s
    }

    /// In-memory service backend returning canned data.
    pub struct MockBackend {
        states: Vec<ServiceState>,
        logs: HashMap<String, Vec<String>>,
    }

    impl MockBackend {
        pub fn new(states: Vec<ServiceState>) -> Self {
            Self {
                states,
                logs: HashMap::new(),
            }
        }

        pub fn with_logs(mut self, name: &str, lines: &[&str]) -> Self {
            self.logs.insert(
                name.to_string(),
                lines.iter().map(|s| s.to_string()).collect(),
            );
            self
        }

        fn state_for(&self, name: &str) -> ServiceState {
            self.states
                .iter()
                .find(|s| s.service_name == name)
                .cloned()
                .unwrap_or_else(|| ServiceState::new(name.to_string()))
        }
    }

    // `ServiceBackend` is declared with `#[async_trait]`, so each method is
    // really a function returning a boxed future. Implementing the expanded
    // form by hand avoids adding an `async-trait` dev-dependency.
    impl ServiceBackend for MockBackend {
        fn start_all<'l, 'f>(&'l self) -> BoxFuture<'f, CoreResult<Vec<ServiceState>>>
        where
            'l: 'f,
            Self: 'f,
        {
            let mut states = self.states.clone();
            for s in &mut states {
                s.status = ServiceStatus::Running;
                s.pid = Some(4242);
            }
            Box::pin(async move { Ok(states) })
        }

        fn start_one<'l, 'n, 'f>(&'l self, name: &'n str) -> BoxFuture<'f, CoreResult<ServiceState>>
        where
            'l: 'f,
            'n: 'f,
            Self: 'f,
        {
            let mut s = self.state_for(name);
            s.status = ServiceStatus::Running;
            s.pid = Some(4242);
            Box::pin(async move { Ok(s) })
        }

        fn stop_all<'l, 'f>(&'l self) -> BoxFuture<'f, CoreResult<()>>
        where
            'l: 'f,
            Self: 'f,
        {
            Box::pin(async { Ok(()) })
        }

        fn stop_one<'l, 'n, 'f>(&'l self, _name: &'n str) -> BoxFuture<'f, CoreResult<()>>
        where
            'l: 'f,
            'n: 'f,
            Self: 'f,
        {
            Box::pin(async { Ok(()) })
        }

        fn get_states<'l, 'f>(&'l self) -> BoxFuture<'f, CoreResult<Vec<ServiceState>>>
        where
            'l: 'f,
            Self: 'f,
        {
            let states = self.states.clone();
            Box::pin(async move { Ok(states) })
        }

        fn get_state<'l, 'n, 'f>(&'l self, name: &'n str) -> BoxFuture<'f, CoreResult<ServiceState>>
        where
            'l: 'f,
            'n: 'f,
            Self: 'f,
        {
            let s = self.state_for(name);
            Box::pin(async move { Ok(s) })
        }

        fn refresh_status<'l, 'f>(&'l self) -> BoxFuture<'f, CoreResult<()>>
        where
            'l: 'f,
            Self: 'f,
        {
            Box::pin(async { Ok(()) })
        }

        fn get_logs<'l, 'n, 'f>(&'l self, name: &'n str) -> BoxFuture<'f, CoreResult<Vec<String>>>
        where
            'l: 'f,
            'n: 'f,
            Self: 'f,
        {
            let logs = self.logs.get(name).cloned().unwrap_or_default();
            Box::pin(async move { Ok(logs) })
        }
    }

    /// Build a `ProjectEntry` backed by a `MockBackend`.
    pub fn project(name: &str, services: &[(&str, ServiceStatus)]) -> ProjectEntry {
        let service_names: Vec<String> = services.iter().map(|(n, _)| n.to_string()).collect();
        let states: Vec<ServiceState> = services.iter().map(|(n, st)| state(n, *st)).collect();
        let service_targets: HashMap<String, Target> = service_names
            .iter()
            .map(|n| (n.clone(), Target::Windows))
            .collect();
        let logs: HashMap<String, Vec<String>> = service_names
            .iter()
            .map(|n| (n.clone(), Vec::new()))
            .collect();

        ProjectEntry {
            name: name.to_string(),
            path: format!("C:\\fixtures\\{name}"),
            backend: Box::new(MockBackend::new(states.clone())),
            service_names: service_names.clone(),
            service_targets,
            service_dirs: vec![None; service_names.len()],
            states,
            logs,
            deps: Vec::new(),
            deps_checked: false,
        }
    }

    /// Two-project fixture: "alpha" (web running, api stopped) and
    /// "beta" (worker stopped).
    pub fn sample_app() -> App {
        App::new(vec![
            project(
                "alpha",
                &[
                    ("web", ServiceStatus::Running),
                    ("api", ServiceStatus::Stopped),
                ],
            ),
            project("beta", &[("worker", ServiceStatus::Stopped)]),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{MockBackend, project, sample_app, state};
    use super::*;

    #[test]
    fn test_new_app_has_expected_defaults() {
        let app = sample_app();
        assert_eq!(app.selected_project, 0);
        assert_eq!(app.selected_service, 0);
        assert_eq!(app.focus, FocusPanel::Projects);
        assert_eq!(app.active_tab, AppTab::Services);
        assert!(!app.show_help);
        assert!(!app.should_quit);
        assert!(app.status_message.is_none());
        assert_eq!(app.log_scroll, 0);
        assert_eq!(app.lang, Lang::Es);
        assert!(app.analysis_result.is_none());
        assert!(!app.log_filter_active);
    }

    #[test]
    fn test_empty_app_accessors_are_safe() {
        let app = App::new(vec![]);
        assert!(app.current_project().is_none());
        assert!(app.selected_service_name().is_none());
        assert!(app.selected_logs().is_empty());
        assert_eq!(app.total_running(), 0);
        assert_eq!(app.total_services(), 0);
        assert_eq!(app.current_total(), 0);
    }

    #[test]
    fn test_current_project_and_service_accessors() {
        let app = sample_app();
        assert_eq!(app.current_project().unwrap().name, "alpha");
        assert_eq!(app.selected_service_name(), Some("web"));
    }

    #[test]
    fn test_service_totals() {
        let app = sample_app();
        assert_eq!(app.total_services(), 3);
        assert_eq!(app.total_running(), 1);
        assert_eq!(app.current_total(), 2);
    }

    #[test]
    fn test_selected_logs_returns_service_buffer() {
        let mut app = sample_app();
        app.projects[0]
            .logs
            .get_mut("web")
            .unwrap()
            .push("hello".to_string());
        assert_eq!(app.selected_logs(), ["hello".to_string()]);
    }

    #[test]
    fn test_move_down_in_projects_focus_switches_project_and_resets() {
        let mut app = sample_app();
        app.selected_service = 1;
        app.log_scroll = 7;
        app.analysis_loading = true;
        app.debt_items = Some(vec![]);
        app.suggest_output = Some("cached".to_string());

        app.move_down();

        assert_eq!(app.selected_project, 1);
        assert_eq!(app.selected_service, 0);
        assert_eq!(app.log_scroll, 0);
        // Tab data must be reset when the project changes.
        assert!(!app.analysis_loading);
        assert!(app.debt_items.is_none());
        assert!(app.suggest_output.is_none());
    }

    #[test]
    fn test_project_navigation_respects_bounds() {
        let mut app = sample_app();
        app.move_up();
        assert_eq!(app.selected_project, 0); // already at top

        app.move_down();
        assert_eq!(app.selected_project, 1);
        app.move_down();
        assert_eq!(app.selected_project, 1); // already at bottom
    }

    #[test]
    fn test_service_navigation_respects_bounds() {
        let mut app = sample_app();
        app.focus = FocusPanel::Services;

        app.move_up();
        assert_eq!(app.selected_service, 0); // already at top

        app.move_down();
        assert_eq!(app.selected_service, 1);
        app.move_down();
        assert_eq!(app.selected_service, 1); // alpha only has 2 services

        app.move_up();
        assert_eq!(app.selected_service, 0);
    }

    #[test]
    fn test_service_navigation_resets_log_scroll() {
        let mut app = sample_app();
        app.focus = FocusPanel::Services;
        app.log_scroll = 5;
        app.move_down();
        assert_eq!(app.log_scroll, 0);
    }

    #[test]
    fn test_log_scroll_in_logs_focus() {
        let mut app = sample_app();
        app.focus = FocusPanel::Logs;

        app.move_down();
        app.move_down();
        assert_eq!(app.log_scroll, 2);

        app.move_up();
        assert_eq!(app.log_scroll, 1);
        app.move_up();
        app.move_up();
        assert_eq!(app.log_scroll, 0); // never goes below zero
    }

    #[test]
    fn test_panel_cycling_forward_and_backward() {
        let mut app = sample_app();
        assert_eq!(app.focus, FocusPanel::Projects);

        app.next_panel();
        assert_eq!(app.focus, FocusPanel::Services);
        app.next_panel();
        assert_eq!(app.focus, FocusPanel::Logs);
        app.next_panel();
        assert_eq!(app.focus, FocusPanel::Projects);

        app.prev_panel();
        assert_eq!(app.focus, FocusPanel::Logs);
        app.prev_panel();
        assert_eq!(app.focus, FocusPanel::Services);
        app.prev_panel();
        assert_eq!(app.focus, FocusPanel::Projects);
    }

    #[test]
    fn test_reset_tab_data_clears_cached_state() {
        let mut app = sample_app();
        app.analysis_loading = true;
        app.audit_loading = true;
        app.debt_items = Some(vec![]);
        app.debt_loading = true;
        app.space_entries = Some(vec![]);
        app.space_loading = true;
        app.stats_loading = true;
        app.search_input.push_str("query");
        app.search_active = true;
        app.search_loading = true;
        app.index_exists = true;
        app.indexing = true;
        app.suggest_output = Some("s".to_string());
        app.suggesting = true;

        app.reset_tab_data();

        assert!(!app.analysis_loading);
        assert!(!app.audit_loading);
        assert!(app.debt_items.is_none());
        assert!(!app.debt_loading);
        assert!(app.space_entries.is_none());
        assert!(!app.space_loading);
        assert!(app.stats_report.is_none());
        assert!(!app.stats_loading);
        assert!(app.search_input.is_empty());
        assert!(!app.search_active);
        assert!(!app.search_loading);
        assert!(!app.index_exists);
        assert!(!app.indexing);
        assert!(app.suggest_output.is_none());
        assert!(!app.suggesting);
    }

    #[tokio::test]
    async fn test_refresh_current_pulls_states_and_appends_new_logs() {
        let mut entry = project("alpha", &[("web", ServiceStatus::Stopped)]);
        // Backend reports the service running and has more log lines than
        // the local buffer.
        entry.backend = Box::new(
            MockBackend::new(vec![state("web", ServiceStatus::Running)])
                .with_logs("web", &["l1", "l2", "l3"]),
        );
        entry.logs.insert("web".to_string(), vec!["l1".to_string()]);
        let mut app = App::new(vec![entry]);

        app.refresh_current().await;

        let project = app.current_project().unwrap();
        assert_eq!(project.states[0].status, ServiceStatus::Running);
        // Only the lines beyond the current buffer length are appended.
        assert_eq!(
            project.logs["web"],
            vec!["l1".to_string(), "l2".to_string(), "l3".to_string()]
        );
    }

    #[tokio::test]
    async fn test_start_all_reports_result_and_logs_transitions() {
        let mut app = sample_app();
        app.start_all().await;

        assert_eq!(
            app.status_message.as_deref(),
            Some("alpha: 2 started, 0 failed")
        );
        let logs = &app.current_project().unwrap().logs["web"];
        assert!(
            logs.iter()
                .any(|l| l.contains("[void-stack] web -> RUNNING")),
            "expected transition log line, got {logs:?}"
        );
    }

    #[tokio::test]
    async fn test_start_selected_and_stop_selected_update_status() {
        let mut app = sample_app();

        app.start_selected().await;
        assert!(
            app.status_message
                .as_deref()
                .unwrap()
                .contains("web started"),
            "unexpected status: {:?}",
            app.status_message
        );

        app.stop_selected().await;
        assert_eq!(app.status_message.as_deref(), Some("web stopped"));
    }

    #[tokio::test]
    async fn test_stop_all_updates_status() {
        let mut app = sample_app();
        app.stop_all().await;
        assert_eq!(
            app.status_message.as_deref(),
            Some("alpha: all services stopped")
        );
    }
}
