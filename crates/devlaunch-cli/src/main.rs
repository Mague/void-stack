use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use devlaunch_core::backend::ServiceBackend;
use devlaunch_core::config;
use devlaunch_core::manager::ProcessManager;
use devlaunch_core::model::ServiceStatus;
use devlaunch_proto::client::DaemonClient;

const DEFAULT_DAEMON_PORT: u16 = 50051;

#[derive(Parser)]
#[command(name = "devlaunch", about = "Unified dev service launcher & monitor")]
struct Cli {
    /// Path to project directory (defaults to current dir)
    #[arg(short, long, default_value = ".")]
    path: String,

    /// Connect to daemon instead of managing processes directly
    #[arg(long)]
    daemon: bool,

    /// Daemon port (used with --daemon)
    #[arg(long, default_value_t = DEFAULT_DAEMON_PORT)]
    port: u16,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start all services (or a specific one)
    Start {
        /// Service name to start (omit for all)
        service: Option<String>,
    },

    /// Stop all services (or a specific one)
    Stop {
        /// Service name to stop (omit for all)
        service: Option<String>,
    },

    /// Show status of all services
    Status,

    /// Initialize a new devlaunch.toml in the current directory
    Init,

    /// Auto-detect project type
    Detect,

    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon for the current project
    Start {
        /// gRPC listen port
        #[arg(long, default_value_t = DEFAULT_DAEMON_PORT)]
        port: u16,
    },
    /// Stop the running daemon
    Stop,
    /// Check daemon status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => cmd_init(&cli.path)?,
        Commands::Detect => cmd_detect(&cli.path),
        Commands::Start { ref service } => {
            let backend = get_backend(&cli).await?;
            cmd_start(backend, service.as_deref()).await?;
        }
        Commands::Stop { ref service } => {
            let backend = get_backend(&cli).await?;
            cmd_stop(backend, service.as_deref()).await?;
        }
        Commands::Status => {
            if cli.daemon {
                cmd_daemon_status().await?;
            } else {
                cmd_status(&cli.path).await?;
            }
        }
        Commands::Daemon { action } => match action {
            DaemonAction::Start { port } => cmd_daemon_start(&cli.path, port).await?,
            DaemonAction::Stop => cmd_daemon_stop().await?,
            DaemonAction::Status => cmd_daemon_status().await?,
        },
    }

    Ok(())
}

/// Get the appropriate backend: DaemonClient if --daemon, otherwise direct ProcessManager.
async fn get_backend(cli: &Cli) -> Result<Box<dyn ServiceBackend>> {
    if cli.daemon {
        let addr = format!("http://127.0.0.1:{}", cli.port);
        let client = DaemonClient::connect_with_timeout(&addr, Duration::from_secs(5))
            .await
            .context("Cannot connect to daemon. Is it running? Start with: devlaunch daemon start")?;
        Ok(Box::new(client))
    } else {
        let project = config::load_project(Path::new(&cli.path))
            .context("Failed to load devlaunch.toml — run 'devlaunch init' first")?;
        Ok(Box::new(ProcessManager::new(project)))
    }
}

fn cmd_init(path: &str) -> Result<()> {
    use devlaunch_core::model::*;

    let dir = Path::new(path);
    let project_type = config::detect_project_type(dir);

    let project = Project {
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "my-project".to_string()),
        description: String::new(),
        path: path.to_string(),
        project_type: Some(project_type),
        tags: vec![],
        services: vec![Service {
            name: "main".to_string(),
            command: default_command_for(project_type),
            target: Target::Windows,
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
        }],
        hooks: Some(HookConfig {
            venv: project_type == ProjectType::Python,
            install_deps: true,
            build: false,
            custom: vec![],
        }),
    };

    config::save_project(&project, dir)?;
    println!("Created devlaunch.toml ({:?} project detected)", project_type);
    Ok(())
}

fn cmd_detect(path: &str) {
    let pt = config::detect_project_type(Path::new(path));
    println!("Detected project type: {:?}", pt);
}

async fn cmd_start(backend: Box<dyn ServiceBackend>, service: Option<&str>) -> Result<()> {
    match service {
        Some(name) => {
            let state = backend.start_one(name).await?;
            println!(
                "  {} {} (pid: {:?})",
                status_icon(&state.status),
                state.service_name,
                state.pid
            );
        }
        None => {
            let states = backend.start_all().await?;
            for state in &states {
                println!(
                    "  {} {} (pid: {:?})",
                    status_icon(&state.status),
                    state.service_name,
                    state.pid,
                );
            }
            println!(
                "\n  {} services started. Press Ctrl+C to stop all.",
                states.len()
            );

            tokio::signal::ctrl_c().await?;
            println!("\nStopping all services...");
            backend.stop_all().await?;
            println!("Done.");
        }
    }

    Ok(())
}

async fn cmd_stop(backend: Box<dyn ServiceBackend>, service: Option<&str>) -> Result<()> {
    match service {
        Some(name) => {
            backend.stop_one(name).await?;
            println!("Stopped: {}", name);
        }
        None => {
            backend.stop_all().await?;
            println!("All services stopped.");
        }
    }
    Ok(())
}

async fn cmd_status(path: &str) -> Result<()> {
    let project = config::load_project(Path::new(path))?;
    println!("Project: {}", project.name);
    println!("Path: {}", project.path);

    if let Some(pt) = project.project_type {
        println!("Type: {:?}", pt);
    }

    println!("\nServices:");
    for service in &project.services {
        println!(
            "  {} [{}] {}",
            if service.enabled { "+" } else { "-" },
            service.target,
            service.name,
        );
        println!("    cmd: {}", service.command);
    }

    Ok(())
}

async fn cmd_daemon_start(path: &str, port: u16) -> Result<()> {
    println!("Starting daemon on port {}...", port);
    println!("Note: Use 'devlaunch-daemon start -p {} --port {}' for the full daemon.", path, port);
    println!("Or run in background:");
    println!("  devlaunch-daemon start -p \"{}\" --port {} &", path, port);
    Ok(())
}

async fn cmd_daemon_stop() -> Result<()> {
    let addr = format!("http://127.0.0.1:{}", DEFAULT_DAEMON_PORT);
    match DaemonClient::connect_with_timeout(&addr, Duration::from_secs(3)).await {
        Ok(mut client) => {
            client.shutdown().await?;
            println!("Daemon shutdown initiated.");
        }
        Err(_) => {
            println!("No daemon is running (cannot connect on port {}).", DEFAULT_DAEMON_PORT);
        }
    }
    Ok(())
}

async fn cmd_daemon_status() -> Result<()> {
    let addr = format!("http://127.0.0.1:{}", DEFAULT_DAEMON_PORT);
    match DaemonClient::connect_with_timeout(&addr, Duration::from_secs(3)).await {
        Ok(mut client) => {
            let info = client.ping().await?;
            println!("DevLaunch Daemon v{}", info.version);
            println!("  Project:  {}", info.project_name);
            println!("  Uptime:   {}s", info.uptime_secs);
            println!("  Services: {}/{} running", info.services_running, info.services_total);
        }
        Err(_) => {
            println!("No daemon is running (cannot connect on port {}).", DEFAULT_DAEMON_PORT);
        }
    }
    Ok(())
}

fn status_icon(status: &ServiceStatus) -> &'static str {
    match status {
        ServiceStatus::Running => "●",
        ServiceStatus::Stopped => "○",
        ServiceStatus::Starting => "◐",
        ServiceStatus::Failed => "✗",
        ServiceStatus::Stopping => "◑",
    }
}

fn default_command_for(pt: devlaunch_core::model::ProjectType) -> String {
    use devlaunch_core::model::ProjectType;
    match pt {
        ProjectType::Python => "python main.py".to_string(),
        ProjectType::Node => "npm run dev".to_string(),
        ProjectType::Rust => "cargo run".to_string(),
        ProjectType::Go => "go run .".to_string(),
        ProjectType::Docker => "docker compose up".to_string(),
        ProjectType::Unknown => "echo 'hello from devlaunch'".to_string(),
    }
}
