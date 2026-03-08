mod app;
mod ui;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use devlaunch_core::backend::ServiceBackend;
use devlaunch_core::config::load_project;
use devlaunch_core::manager::ProcessManager;
use devlaunch_core::model::Target;
use devlaunch_proto::client::DaemonClient;

use app::{App, FocusPanel};

const DEFAULT_DAEMON_PORT: u16 = 50051;

/// DevLaunch TUI - real-time service dashboard.
#[derive(Parser, Debug)]
#[command(name = "devlaunch-tui", about = "TUI dashboard for DevLaunch")]
struct Cli {
    /// Path to devlaunch.toml or the directory containing it.
    #[arg(short, long, default_value = ".")]
    path: PathBuf,

    /// Connect to daemon instead of managing processes directly
    #[arg(long)]
    daemon: bool,

    /// Daemon port (used with --daemon)
    #[arg(long, default_value_t = DEFAULT_DAEMON_PORT)]
    port: u16,
}

const TICK_RATE: Duration = Duration::from_secs(1);
const POLL_TIMEOUT: Duration = Duration::from_millis(100);

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing to a file so it doesn't interfere with the TUI
    let log_file = std::fs::File::create("devlaunch-tui.log").ok();
    if let Some(file) = log_file {
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_ansi(false)
            .init();
    }

    // Build backend and app based on mode
    let (backend, project_name, service_names, service_targets, daemon_mode): (
        Box<dyn ServiceBackend>,
        String,
        Vec<String>,
        HashMap<String, Target>,
        bool,
    ) = if cli.daemon {
        // Daemon mode: connect via gRPC
        let addr = format!("http://127.0.0.1:{}", cli.port);
        let mut client = DaemonClient::connect_with_timeout(&addr, Duration::from_secs(5))
            .await
            .context("Cannot connect to daemon. Is it running?")?;

        let ping = client.ping().await?;
        let project_name = ping.project_name;

        // Get service names from current states
        let states = client.get_states().await?;
        let service_names: Vec<String> = states.iter().map(|s| s.service_name.clone()).collect();
        let service_targets: HashMap<String, Target> = HashMap::new(); // targets not available in daemon mode

        (Box::new(client), project_name, service_names, service_targets, true)
    } else {
        // Direct mode: manage processes locally
        let project = load_project(&cli.path)
            .with_context(|| format!("Failed to load project from {}", cli.path.display()))?;

        let project_name = project.name.clone();
        let service_names: Vec<String> = project.services.iter().map(|s| s.name.clone()).collect();
        let service_targets: HashMap<String, Target> = project
            .services
            .iter()
            .map(|s| (s.name.clone(), s.target))
            .collect();
        let manager = ProcessManager::new(project);

        (Box::new(manager), project_name, service_names, service_targets, false)
    };

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend_term = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend_term)?;

    // Create app state
    let mut app = App::new(backend, project_name, service_names, service_targets, daemon_mode);

    // Run the main loop
    let result = run_loop(&mut terminal, &mut app).await;

    // Cleanup: stop all services before exit (only in direct mode)
    if !app.daemon_mode {
        let _ = app.backend.stop_all().await;
    }

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

        // Tick: refresh service status periodically
        if last_tick.elapsed() >= TICK_RATE {
            app.refresh_states().await;
            last_tick = Instant::now();
        }
    }
}

async fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    match app.focus {
        FocusPanel::Services => handle_services_key(app, code, modifiers).await,
        FocusPanel::Logs => handle_logs_key(app, code),
    }
}

async fn handle_services_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    match code {
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Char('?') => {
            app.show_help = true;
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
        KeyCode::Char('k') if !modifiers.contains(KeyModifiers::SHIFT) => {
            app.stop_selected().await;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_down();
        }
        KeyCode::Up => {
            app.move_up();
        }
        KeyCode::Char('l') => {
            app.focus = FocusPanel::Logs;
            app.log_scroll = 0;
        }
        KeyCode::Char('r') => {
            app.refresh_states().await;
            app.status_message = Some("Status refreshed".to_string());
        }
        KeyCode::Esc => {
            app.status_message = None;
        }
        _ => {}
    }
}

fn handle_logs_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Char('?') => {
            app.show_help = true;
        }
        KeyCode::Esc | KeyCode::Char('l') => {
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
