mod app;
mod i18n;
mod ui;

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use void_stack_core::global_config::load_global_config;
use void_stack_core::manager::ProcessManager;
use void_stack_core::model::Target;

use app::{App, AppTab, FocusPanel, ProjectEntry};

/// VoidStack TUI - multi-project service dashboard.
#[derive(Parser, Debug)]
#[command(name = "void-tui", about = "TUI dashboard for VoidStack")]
struct Cli {
    /// Start only a specific project (by default shows all)
    #[arg()]
    project: Option<String>,
}

const TICK_RATE: Duration = Duration::from_secs(1);
const POLL_TIMEOUT: Duration = Duration::from_millis(100);

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing to a file so it doesn't interfere with the TUI
    let log_file = std::fs::File::create("void-tui.log").ok();
    if let Some(file) = log_file {
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_ansi(false)
            .init();
    }

    // Load all projects from global config
    let config = load_global_config().context("Failed to load global config")?;

    if config.projects.is_empty() {
        eprintln!("No projects registered. Use 'void add <name> <path>' first.");
        return Ok(());
    }

    // Filter by project name if provided
    let projects_to_load = if let Some(ref name) = cli.project {
        let p = config
            .projects
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Project '{}' not found. Use 'void list' to see registered projects.",
                    name
                )
            })?;
        vec![p]
    } else {
        config.projects.clone()
    };

    // Build a ProjectEntry (with ProcessManager backend) for each project
    let mut entries: Vec<ProjectEntry> = Vec::with_capacity(projects_to_load.len());
    for project in projects_to_load {
        let service_names: Vec<String> = project.services.iter().map(|s| s.name.clone()).collect();
        let service_targets: HashMap<String, Target> = project
            .services
            .iter()
            .map(|s| (s.name.clone(), s.target))
            .collect();
        let logs: HashMap<String, Vec<String>> = service_names
            .iter()
            .map(|n| (n.clone(), Vec::new()))
            .collect();

        let name = project.name.clone();
        let path = project.path.clone();
        let service_dirs: Vec<Option<String>> = project
            .services
            .iter()
            .map(|s| s.working_dir.clone())
            .collect();
        let states = service_names
            .iter()
            .map(|n| void_stack_core::model::ServiceState::new(n.clone()))
            .collect();

        let manager = ProcessManager::new(project);

        entries.push(ProjectEntry {
            name,
            path,
            backend: Box::new(manager),
            service_names,
            service_targets,
            service_dirs,
            states,
            logs,
            deps: vec![],
            deps_checked: false,
        });
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend_term = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend_term)?;

    // Create app state
    let mut app = App::new(entries);

    // Run the main loop
    let result = run_loop(&mut terminal, &mut app).await;

    // Stop all services across all projects before exit
    app.stop_everything().await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let mut last_tick = Instant::now();

    loop {
        // Draw
        terminal.draw(|f| ui::draw(f, app))?;

        // Poll for keyboard events
        if event::poll(POLL_TIMEOUT)?
            && let Event::Key(key) = event::read()?
        {
            // On Windows, crossterm reports Press + Release for each key.
            // Only handle Press events to avoid double-firing.
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Ctrl+C always quits
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                app.should_quit = true;
            }

            // Help overlay intercepts all keys
            if app.show_help {
                app.show_help = false;
            } else {
                handle_key(app, key.code, key.modifiers).await;
            }
        }

        if app.should_quit {
            return Ok(());
        }

        // Tick: refresh current project status periodically
        if last_tick.elapsed() >= TICK_RATE {
            app.refresh_current().await;
            last_tick = Instant::now();
        }
    }
}

async fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // Global keys
    match code {
        KeyCode::Char('q') => {
            app.should_quit = true;
            return;
        }
        KeyCode::Char('?') => {
            app.show_help = true;
            return;
        }
        // Tab switching: 1-5
        KeyCode::Char('1') => {
            app.active_tab = AppTab::Services;
            return;
        }
        KeyCode::Char('2') => {
            app.active_tab = AppTab::Analysis;
            return;
        }
        KeyCode::Char('3') => {
            app.active_tab = AppTab::Security;
            return;
        }
        KeyCode::Char('4') => {
            app.active_tab = AppTab::Debt;
            return;
        }
        KeyCode::Char('5') => {
            app.active_tab = AppTab::Space;
            return;
        }
        KeyCode::Char('6') => {
            app.active_tab = AppTab::Stats;
            return;
        }
        // L = Toggle language (ES/EN)
        KeyCode::Char('L') => {
            app.lang = app.lang.toggle();
            app.status_message = Some(format!("Language: {}", app.lang.code()));
            return;
        }
        // R = Run action for the current tab
        KeyCode::Char('R') => {
            run_tab_action(app).await;
            return;
        }
        KeyCode::Tab => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_panel();
            } else {
                app.next_panel();
            }
            return;
        }
        KeyCode::BackTab => {
            app.prev_panel();
            return;
        }
        _ => {}
    }

    // Dispatch to tab-specific handler
    match app.active_tab {
        AppTab::Services => match app.focus {
            FocusPanel::Projects => handle_projects_key(app, code, modifiers).await,
            FocusPanel::Services => handle_services_key(app, code, modifiers).await,
            FocusPanel::Logs => handle_logs_key(app, code),
        },
        AppTab::Analysis => handle_analysis_key(app, code).await,
        AppTab::Security | AppTab::Debt | AppTab::Space | AppTab::Stats => {
            // Non-Services tabs: j/k navigates projects
            navigate_projects(app, code);
        }
    }
}

fn navigate_projects(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            let max = app.projects.len();
            if max > 0 && app.selected_project < max - 1 {
                app.selected_project += 1;
                app.selected_service = 0;
                app.log_scroll = 0;
                app.reset_tab_data();
            }
        }
        KeyCode::Char('k') | KeyCode::Up if app.selected_project > 0 => {
            app.selected_project -= 1;
            app.selected_service = 0;
            app.log_scroll = 0;
            app.reset_tab_data();
        }
        _ => {}
    }
}

async fn handle_analysis_key(app: &mut App, code: KeyCode) {
    // Search input mode takes priority
    #[cfg(feature = "vector")]
    if app.search_active {
        handle_search_input(app, code);
        return;
    }

    match code {
        #[cfg(feature = "vector")]
        KeyCode::Char('/') => action_start_search(app),
        #[cfg(feature = "vector")]
        KeyCode::Char('I') => action_index_project(app),
        #[cfg(feature = "vector")]
        KeyCode::Char('G') => action_generate_voidignore(app),
        KeyCode::Char('U') => action_suggest(app).await,
        _ => navigate_projects(app, code),
    }
}

#[cfg(feature = "vector")]
fn action_start_search(app: &mut App) {
    app.search_active = true;
    app.search_input.clear();
}

#[cfg(feature = "vector")]
fn handle_search_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.search_active = false;
            app.search_input.clear();
        }
        KeyCode::Enter => {
            action_run_search(app);
            app.search_active = false;
        }
        KeyCode::Backspace => {
            app.search_input.pop();
        }
        KeyCode::Char(c) => {
            app.search_input.push(c);
        }
        _ => {}
    }
}

#[cfg(feature = "vector")]
fn action_run_search(app: &mut App) {
    if app.search_input.is_empty() {
        return;
    }
    let query = app.search_input.clone();
    let Some(project) = app.current_project() else {
        return;
    };
    let project_clone = void_stack_core::model::Project {
        name: project.name.clone(),
        path: project.path.clone(),
        description: String::new(),
        project_type: None,
        tags: vec![],
        services: vec![],
        hooks: None,
    };
    app.search_loading = true;
    match void_stack_core::vector_index::semantic_search(&project_clone, &query, 5) {
        Ok(results) => {
            let count = results.len();
            app.search_results = Some(results);
            app.status_message = Some(format!("{} results for \"{}\"", count, query));
        }
        Err(e) => {
            app.status_message = Some(format!("Search error: {}", e));
        }
    }
    app.search_loading = false;
}

#[cfg(feature = "vector")]
fn action_index_project(app: &mut App) {
    let Some(project) = app.current_project() else {
        return;
    };
    let project_clone = void_stack_core::model::Project {
        name: project.name.clone(),
        path: project.path.clone(),
        description: String::new(),
        project_type: None,
        tags: vec![],
        services: vec![],
        hooks: None,
    };
    app.indexing = true;
    app.status_message = Some(i18n::t(app.lang, "search.indexing").to_string());
    match void_stack_core::vector_index::index_project(&project_clone, false, None, |_, _| {}) {
        Ok(stats) => {
            app.index_exists = true;
            app.status_message = Some(format!(
                "✓ Index: {} files, {} chunks ({:.1}MB)",
                stats.files_indexed, stats.chunks_total, stats.size_mb
            ));
        }
        Err(e) => {
            app.status_message = Some(format!("Index error: {}", e));
        }
    }
    app.indexing = false;
}

#[cfg(feature = "vector")]
fn action_generate_voidignore(app: &mut App) {
    let Some(proj_path) = app.current_project().map(|p| p.path.clone()) else {
        return;
    };
    let path = std::path::Path::new(&proj_path);
    app.status_message = Some(i18n::t(app.lang, "voidignore.generating").to_string());
    let result = void_stack_core::vector_index::generate_voidignore(path);
    match void_stack_core::vector_index::save_voidignore(path, &result.content) {
        Ok(_) => {
            app.status_message = Some(format!(
                "✓ .voidignore ({} {})",
                result.patterns_count,
                i18n::t(app.lang, "voidignore.patterns"),
            ));
        }
        Err(e) => {
            app.status_message = Some(format!("Error: {}", e));
        }
    }
}

async fn action_suggest(app: &mut App) {
    if app.suggesting {
        return;
    }
    let Some((proj_name, proj_path)) = app
        .current_project()
        .map(|p| (p.name.clone(), p.path.clone()))
    else {
        return;
    };

    app.suggesting = true;
    app.suggest_output = None;
    app.status_message = Some(i18n::t(app.lang, "suggest.running").to_string());

    let path = void_stack_core::runner::local::strip_win_prefix(&proj_path);
    let analysis_path = std::path::Path::new(&path);

    if let Some(result) = void_stack_core::analyzer::analyze_project(analysis_path) {
        let ai_config = void_stack_core::ai::load_ai_config().unwrap_or_default();
        let project_model = void_stack_core::model::Project {
            name: proj_name,
            path: proj_path,
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        match void_stack_core::ai::suggest_with_project(&ai_config, &result, &project_model).await {
            Ok(sr) => {
                let text = if sr.suggestions.is_empty() {
                    sr.raw_response.clone()
                } else {
                    sr.suggestions
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            format!(
                                "{}. [{}] {}\n   {}",
                                i + 1,
                                s.priority,
                                s.title,
                                s.description
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n")
                };
                app.suggest_output = Some(text);
                app.status_message = Some(format!(
                    "✓ {} {}",
                    sr.suggestions.len(),
                    i18n::t(app.lang, "suggest.generated"),
                ));
            }
            Err(e) => {
                let context =
                    void_stack_core::ai::build_context_with_project(&result, &project_model);
                app.suggest_output = Some(context);
                app.status_message = Some(i18n::t(app.lang, "suggest.no_ollama").to_string());
                let _ = e;
            }
        }
    } else {
        app.status_message = Some(i18n::t(app.lang, "analysis.no_code").to_string());
    }
    app.suggesting = false;
}

/// Run the appropriate action for the currently active tab.
async fn run_tab_action(app: &mut App) {
    let project_path = match app.current_project() {
        Some(p) => void_stack_core::runner::local::strip_win_prefix(&p.path),
        None => return,
    };
    let path = std::path::Path::new(&project_path);

    let l = app.lang;
    match app.active_tab {
        AppTab::Analysis => {
            app.analysis_loading = true;
            app.status_message = Some(i18n::t(l, "analysis.running").to_string());
            let result = void_stack_core::analyzer::analyze_project(path);
            app.analysis_result = result;
            app.analysis_loading = false;
            app.status_message = Some(i18n::t(l, "analysis.complete").to_string());
        }
        AppTab::Security => {
            app.audit_loading = true;
            app.status_message = Some(i18n::t(l, "security.running").to_string());
            let project_name = app
                .current_project()
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let result = void_stack_core::audit::audit_project(&project_name, path);
            app.audit_result = Some(result);
            app.audit_loading = false;
            app.status_message = Some(i18n::t(l, "security.complete").to_string());
        }
        AppTab::Debt => {
            app.debt_loading = true;
            app.status_message = Some(i18n::t(l, "debt.running").to_string());
            let items = void_stack_core::analyzer::explicit_debt::scan_explicit_debt(path);
            let count = items.len();
            app.debt_items = Some(items);
            app.debt_loading = false;
            app.status_message = Some(format!("{} {}", count, i18n::t(l, "debt.found")));
        }
        AppTab::Space => {
            app.space_loading = true;
            app.status_message = Some(i18n::t(l, "space.running").to_string());
            let project_entries = void_stack_core::space::scan_project(path);
            let global_entries = void_stack_core::space::scan_global();
            let mut entries: Vec<void_stack_core::space::SpaceEntry> =
                Vec::with_capacity(project_entries.len() + global_entries.len());
            entries.extend(project_entries);
            entries.extend(global_entries);
            entries.sort_by_key(|b| std::cmp::Reverse(b.size_bytes));
            let count = entries.len();
            app.space_entries = Some(entries);
            app.space_loading = false;
            app.status_message = Some(format!("{} {}", count, i18n::t(l, "space.found")));
        }
        AppTab::Stats => {
            app.stats_loading = true;
            app.status_message = Some(i18n::t(l, "stats.running").to_string());
            let project_name = app.current_project().map(|p| p.name.clone());
            match void_stack_core::stats::get_stats(project_name.as_deref(), 30) {
                Ok(report) => {
                    let total = report.total_operations;
                    app.stats_report = Some(report);
                    app.status_message =
                        Some(format!("{} {}", total, i18n::t(l, "stats.ops_found")));
                }
                Err(e) => {
                    app.status_message = Some(format!("Error: {}", e));
                }
            }
            app.stats_loading = false;
        }
        AppTab::Services => {}
    }
}

fn generate_claudeignore(app: &mut App) {
    let project_path = match app.current_project() {
        Some(p) => void_stack_core::runner::local::strip_win_prefix(&p.path),
        None => return,
    };
    let path = std::path::Path::new(&project_path);
    let l = app.lang;

    let result = void_stack_core::claudeignore::generate_claudeignore(path);
    match void_stack_core::claudeignore::save_claudeignore(path, &result.content) {
        Ok(saved_path) => {
            app.status_message = Some(format!(
                "✓ {} {} — {} {} | ~{} {}",
                i18n::t(l, "claudeignore.generated"),
                saved_path.display(),
                result.patterns_count,
                i18n::t(l, "claudeignore.patterns"),
                result.estimated_files_ignored,
                i18n::t(l, "claudeignore.files_ignored"),
            ));
        }
        Err(e) => {
            app.status_message = Some(format!("✗ {}: {}", i18n::t(l, "claudeignore.error"), e));
        }
    }
}

async fn handle_projects_key(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) {
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_down();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_up();
        }
        KeyCode::Char('a') | KeyCode::Enter => {
            app.start_all().await;
        }
        KeyCode::Char('K') => {
            app.stop_all().await;
        }
        KeyCode::Char('r') => {
            app.refresh_all().await;
            app.status_message = Some(i18n::t(app.lang, "status.all_refreshed").to_string());
        }
        KeyCode::Char('G') => {
            generate_claudeignore(app);
        }
        KeyCode::Char('d') => {
            app.check_deps().await;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.focus = FocusPanel::Services;
        }
        _ => {}
    }
}

async fn handle_services_key(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) {
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_down();
        }
        KeyCode::Up => {
            app.move_up();
        }
        KeyCode::Char('a') | KeyCode::Enter => {
            app.start_all().await;
        }
        KeyCode::Char('s') => {
            app.start_selected().await;
        }
        KeyCode::Char('K') => {
            app.stop_all().await;
        }
        KeyCode::Char('k') => {
            app.stop_selected().await;
        }
        KeyCode::Char('l') => {
            app.focus = FocusPanel::Logs;
            app.log_scroll = 0;
        }
        KeyCode::Char('d') => {
            app.check_deps().await;
        }
        KeyCode::Char('r') => {
            app.refresh_current().await;
            app.status_message = Some(i18n::t(app.lang, "status.refreshed").to_string());
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.focus = FocusPanel::Projects;
        }
        KeyCode::Esc => {
            app.status_message = None;
        }
        _ => {}
    }
}

fn handle_logs_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            app.focus = FocusPanel::Services;
        }
        KeyCode::Char('f') => {
            app.log_filter_active = !app.log_filter_active;
            app.log_filter_savings = None;
            let l = app.lang;
            app.status_message = Some(if app.log_filter_active {
                i18n::t(l, "logs.filter_on").to_string()
            } else {
                i18n::t(l, "logs.filter_off").to_string()
            });
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_down();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_up();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::sample_app;
    use crate::i18n::Lang;

    const NONE: KeyModifiers = KeyModifiers::NONE;

    #[tokio::test]
    async fn test_q_sets_quit_flag() {
        let mut app = sample_app();
        handle_key(&mut app, KeyCode::Char('q'), NONE).await;
        assert!(app.should_quit);
    }

    #[tokio::test]
    async fn test_question_mark_opens_help_overlay() {
        let mut app = sample_app();
        handle_key(&mut app, KeyCode::Char('?'), NONE).await;
        assert!(app.show_help);
    }

    #[tokio::test]
    async fn test_number_keys_switch_tabs() {
        let cases = [
            ('1', AppTab::Services),
            ('2', AppTab::Analysis),
            ('3', AppTab::Security),
            ('4', AppTab::Debt),
            ('5', AppTab::Space),
            ('6', AppTab::Stats),
        ];
        let mut app = sample_app();
        for (ch, tab) in cases {
            handle_key(&mut app, KeyCode::Char(ch), NONE).await;
            assert_eq!(app.active_tab, tab, "key '{ch}' should select {tab:?}");
        }
    }

    #[tokio::test]
    async fn test_uppercase_l_toggles_language() {
        let mut app = sample_app();
        assert_eq!(app.lang, Lang::Es);

        handle_key(&mut app, KeyCode::Char('L'), NONE).await;
        assert_eq!(app.lang, Lang::En);
        assert_eq!(app.status_message.as_deref(), Some("Language: EN"));

        handle_key(&mut app, KeyCode::Char('L'), NONE).await;
        assert_eq!(app.lang, Lang::Es);
    }

    #[tokio::test]
    async fn test_tab_and_backtab_cycle_panels() {
        let mut app = sample_app();
        assert_eq!(app.focus, FocusPanel::Projects);

        handle_key(&mut app, KeyCode::Tab, NONE).await;
        assert_eq!(app.focus, FocusPanel::Services);

        handle_key(&mut app, KeyCode::BackTab, NONE).await;
        assert_eq!(app.focus, FocusPanel::Projects);

        // Shift+Tab also cycles backwards.
        handle_key(&mut app, KeyCode::Tab, KeyModifiers::SHIFT).await;
        assert_eq!(app.focus, FocusPanel::Logs);
    }

    #[tokio::test]
    async fn test_non_services_tab_navigates_projects() {
        let mut app = sample_app();
        app.active_tab = AppTab::Security;

        handle_key(&mut app, KeyCode::Char('j'), NONE).await;
        assert_eq!(app.selected_project, 1);

        handle_key(&mut app, KeyCode::Char('k'), NONE).await;
        assert_eq!(app.selected_project, 0);
    }

    #[test]
    fn test_navigate_projects_respects_bounds_and_resets() {
        let mut app = sample_app();

        navigate_projects(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selected_project, 0); // already at top

        app.debt_items = Some(vec![]);
        navigate_projects(&mut app, KeyCode::Char('j'));
        assert_eq!(app.selected_project, 1);
        assert!(app.debt_items.is_none()); // tab data reset on switch

        navigate_projects(&mut app, KeyCode::Char('j'));
        assert_eq!(app.selected_project, 1); // already at bottom

        navigate_projects(&mut app, KeyCode::Up);
        assert_eq!(app.selected_project, 0);
    }

    #[tokio::test]
    async fn test_services_key_navigation_and_focus_moves() {
        let mut app = sample_app();
        app.focus = FocusPanel::Services;

        handle_services_key(&mut app, KeyCode::Char('j'), NONE).await;
        assert_eq!(app.selected_service, 1);
        handle_services_key(&mut app, KeyCode::Up, NONE).await;
        assert_eq!(app.selected_service, 0);

        handle_services_key(&mut app, KeyCode::Char('l'), NONE).await;
        assert_eq!(app.focus, FocusPanel::Logs);

        app.focus = FocusPanel::Services;
        handle_services_key(&mut app, KeyCode::Char('h'), NONE).await;
        assert_eq!(app.focus, FocusPanel::Projects);

        app.status_message = Some("old".to_string());
        handle_services_key(&mut app, KeyCode::Esc, NONE).await;
        assert!(app.status_message.is_none());
    }

    #[tokio::test]
    async fn test_services_key_start_stop_refresh_via_mock_backend() {
        let mut app = sample_app();
        app.focus = FocusPanel::Services;

        handle_services_key(&mut app, KeyCode::Char('s'), NONE).await;
        assert!(
            app.status_message
                .as_deref()
                .unwrap()
                .contains("web started"),
            "unexpected status: {:?}",
            app.status_message
        );

        handle_services_key(&mut app, KeyCode::Char('k'), NONE).await;
        assert_eq!(app.status_message.as_deref(), Some("web stopped"));

        handle_services_key(&mut app, KeyCode::Char('r'), NONE).await;
        assert_eq!(app.status_message.as_deref(), Some("Estado actualizado"));
    }

    #[tokio::test]
    async fn test_projects_key_focus_and_refresh_all() {
        let mut app = sample_app();

        handle_projects_key(&mut app, KeyCode::Char('l'), NONE).await;
        assert_eq!(app.focus, FocusPanel::Services);

        app.focus = FocusPanel::Projects;
        handle_projects_key(&mut app, KeyCode::Right, NONE).await;
        assert_eq!(app.focus, FocusPanel::Services);

        handle_projects_key(&mut app, KeyCode::Char('r'), NONE).await;
        assert_eq!(
            app.status_message.as_deref(),
            Some("Todos los proyectos actualizados")
        );

        // Project navigation only applies while the Projects panel is focused.
        app.focus = FocusPanel::Projects;
        handle_projects_key(&mut app, KeyCode::Char('j'), NONE).await;
        assert_eq!(app.selected_project, 1);
    }

    #[test]
    fn test_logs_key_escape_and_filter_toggle() {
        let mut app = sample_app();
        app.focus = FocusPanel::Logs;

        handle_logs_key(&mut app, KeyCode::Char('f'));
        assert!(app.log_filter_active);
        assert_eq!(
            app.status_message.as_deref(),
            Some("Filtrado de logs activado")
        );

        handle_logs_key(&mut app, KeyCode::Char('f'));
        assert!(!app.log_filter_active);
        assert_eq!(
            app.status_message.as_deref(),
            Some("Filtrado de logs desactivado")
        );

        handle_logs_key(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, FocusPanel::Services);
    }

    #[cfg(feature = "vector")]
    #[test]
    fn test_search_input_editing_without_running_search() {
        let mut app = sample_app();

        action_start_search(&mut app);
        assert!(app.search_active);

        handle_search_input(&mut app, KeyCode::Char('a'));
        handle_search_input(&mut app, KeyCode::Char('b'));
        assert_eq!(app.search_input, "ab");

        handle_search_input(&mut app, KeyCode::Backspace);
        assert_eq!(app.search_input, "a");

        handle_search_input(&mut app, KeyCode::Esc);
        assert!(!app.search_active);
        assert!(app.search_input.is_empty());

        // Enter with an empty query exits input mode without searching.
        action_start_search(&mut app);
        handle_search_input(&mut app, KeyCode::Enter);
        assert!(!app.search_active);
        assert!(app.search_results.is_none());
    }
}
