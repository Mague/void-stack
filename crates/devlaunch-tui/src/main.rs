mod app;
mod ui;

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

use devlaunch_core::config::load_project;

use app::{App, FocusPanel};

/// DevLaunch TUI - real-time service dashboard.
#[derive(Parser, Debug)]
#[command(name = "devlaunch-tui", about = "TUI dashboard for DevLaunch")]
struct Cli {
    /// Path to devlaunch.toml or the directory containing it.
    #[arg(short, long, default_value = ".")]
    path: PathBuf,
}

const TICK_RATE: Duration = Duration::from_secs(1);
const POLL_TIMEOUT: Duration = Duration::from_millis(100);

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI args
    let cli = Cli::parse();

    // Initialize tracing to a file so it doesn't interfere with the TUI
    let log_file = std::fs::File::create("devlaunch-tui.log").ok();
    if let Some(file) = log_file {
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_ansi(false)
            .init();
    }

    // Load project config
    let project = load_project(&cli.path)
        .with_context(|| format!("Failed to load project from {}", cli.path.display()))?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(project);

    // Run the main loop
    let result = run_loop(&mut terminal, &mut app).await;

    // Cleanup: stop all services before exit
    let _ = app.manager.stop_all().await;

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
                    // Consume the key — don't process further
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
            // Shift+K — stop all
            app.stop_all().await;
        }
        KeyCode::Char('k') if !modifiers.contains(KeyModifiers::SHIFT) => {
            // Lowercase k without shift — stop selected (also move up in vim-style)
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
            // Nothing to close in services panel, but clear status
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
