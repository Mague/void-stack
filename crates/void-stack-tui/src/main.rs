mod app;
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
    let config = load_global_config()
        .context("Failed to load global config")?;

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
            .ok_or_else(|| anyhow::anyhow!("Project '{}' not found. Use 'void list' to see registered projects.", name))?;
        vec![p]
    } else {
        config.projects.clone()
    };

    // Build a ProjectEntry (with ProcessManager backend) for each project
    let mut entries: Vec<ProjectEntry> = Vec::new();
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
        let service_dirs: Vec<Option<String>> = project.services.iter()
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
        if event::poll(POLL_TIMEOUT)? {
            if let Event::Key(key) = event::read()? {
                // On Windows, crossterm reports Press + Release for each key.
                // Only handle Press events to avoid double-firing.
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Ctrl+C always quits
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
                {
                    app.should_quit = true;
                }

                // Help overlay intercepts all keys
                if app.show_help {
                    app.show_help = false;
                } else {
                    handle_key(app, key.code, key.modifiers).await;
                }
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
        KeyCode::Char('1') => { app.active_tab = AppTab::Services; return; }
        KeyCode::Char('2') => { app.active_tab = AppTab::Analysis; return; }
        KeyCode::Char('3') => { app.active_tab = AppTab::Security; return; }
        KeyCode::Char('4') => { app.active_tab = AppTab::Debt; return; }
        KeyCode::Char('5') => { app.active_tab = AppTab::Space; return; }
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

    // Project navigation works on ALL tabs (j/k to switch project)
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if app.active_tab != AppTab::Services || app.focus == FocusPanel::Projects {
                // Navigate projects on non-service tabs, or when focused on Projects panel
                let max = app.projects.len();
                if max > 0 && app.selected_project < max - 1 {
                    app.selected_project += 1;
                    app.selected_service = 0;
                    app.log_scroll = 0;
                    app.reset_tab_data();
                }
                if app.active_tab != AppTab::Services {
                    return;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.active_tab != AppTab::Services || app.focus == FocusPanel::Projects {
                if app.selected_project > 0 {
                    app.selected_project -= 1;
                    app.selected_service = 0;
                    app.log_scroll = 0;
                    app.reset_tab_data();
                }
                if app.active_tab != AppTab::Services {
                    return;
                }
            }
        }
        _ => {}
    }

    // Panel-specific keys (only on Services tab)
    if app.active_tab == AppTab::Services {
        match app.focus {
            FocusPanel::Projects => handle_projects_key(app, code, modifiers).await,
            FocusPanel::Services => handle_services_key(app, code, modifiers).await,
            FocusPanel::Logs => handle_logs_key(app, code),
        }
    }
}

/// Run the appropriate action for the currently active tab.
async fn run_tab_action(app: &mut App) {
    let project_path = match app.current_project() {
        Some(p) => void_stack_core::runner::local::strip_win_prefix(&p.path),
        None => return,
    };
    let path = std::path::Path::new(&project_path);

    match app.active_tab {
        AppTab::Analysis => {
            app.analysis_loading = true;
            app.status_message = Some("Running analysis...".to_string());
            // Run analysis synchronously (fast enough for most projects)
            let result = void_stack_core::analyzer::analyze_project(path);
            app.analysis_result = result;
            app.analysis_loading = false;
            app.status_message = Some("Analysis complete".to_string());
        }
        AppTab::Security => {
            app.audit_loading = true;
            app.status_message = Some("Running security audit...".to_string());
            let project_name = app.current_project().map(|p| p.name.clone()).unwrap_or_default();
            let result = void_stack_core::audit::audit_project(&project_name, path);
            app.audit_result = Some(result);
            app.audit_loading = false;
            app.status_message = Some("Audit complete".to_string());
        }
        AppTab::Debt => {
            app.debt_loading = true;
            app.status_message = Some("Scanning for debt markers...".to_string());
            let items = void_stack_core::analyzer::explicit_debt::scan_explicit_debt(path);
            let count = items.len();
            app.debt_items = Some(items);
            app.debt_loading = false;
            app.status_message = Some(format!("Found {} debt markers", count));
        }
        AppTab::Space => {
            app.space_loading = true;
            app.status_message = Some("Scanning disk space...".to_string());
            let project_entries = void_stack_core::space::scan_project(path);
            let global_entries = void_stack_core::space::scan_global();
            let mut entries: Vec<void_stack_core::space::SpaceEntry> = Vec::new();
            entries.extend(project_entries);
            entries.extend(global_entries);
            entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
            let count = entries.len();
            app.space_entries = Some(entries);
            app.space_loading = false;
            app.status_message = Some(format!("Found {} space entries", count));
        }
        AppTab::Services => {} // No R action on services tab
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
            app.status_message = Some("All projects refreshed".to_string());
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
            app.status_message = Some("Status refreshed".to_string());
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
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_down();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_up();
        }
        _ => {}
    }
}
