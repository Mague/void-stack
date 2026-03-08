use anyhow::Result;
use clap::{Parser, Subcommand};

use devlaunch_core::config;
use devlaunch_core::manager::ProcessManager;

#[derive(Parser)]
#[command(name = "devlaunch", about = "Unified dev service launcher & monitor")]
struct Cli {
    /// Path to project directory (defaults to current dir)
    #[arg(short, long, default_value = ".")]
    path: String,

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
        Commands::Start { service } => cmd_start(&cli.path, service.as_deref()).await?,
        Commands::Stop { service } => cmd_stop(&cli.path, service.as_deref()).await?,
        Commands::Status => cmd_status(&cli.path).await?,
    }

    Ok(())
}

fn cmd_init(path: &str) -> Result<()> {
    use devlaunch_core::model::*;
    use std::path::Path;

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

fn cmd_detect(path: &str) -> () {
    let pt = config::detect_project_type(std::path::Path::new(path));
    println!("Detected project type: {:?}", pt);
}

async fn cmd_start(path: &str, service: Option<&str>) -> Result<()> {
    let project = config::load_project(std::path::Path::new(path))?;
    let manager = ProcessManager::new(project);

    match service {
        Some(name) => {
            let state = manager.start_one(name).await?;
            println!("  {} {} (pid: {:?})", status_icon(&state.status), state.service_name, state.pid);
        }
        None => {
            let states = manager.start_all().await?;
            for state in &states {
                println!(
                    "  {} {} (pid: {:?})",
                    status_icon(&state.status),
                    state.service_name,
                    state.pid,
                );
            }
            println!("\n  {} services started. Press Ctrl+C to stop all.", states.len());

            // Wait for Ctrl+C
            tokio::signal::ctrl_c().await?;
            println!("\nStopping all services...");
            manager.stop_all().await?;
            println!("Done.");
        }
    }

    Ok(())
}

async fn cmd_stop(path: &str, service: Option<&str>) -> Result<()> {
    let project = config::load_project(std::path::Path::new(path))?;
    let manager = ProcessManager::new(project);

    match service {
        Some(name) => {
            manager.stop_one(name).await?;
            println!("Stopped: {}", name);
        }
        None => {
            manager.stop_all().await?;
            println!("All services stopped.");
        }
    }

    Ok(())
}

async fn cmd_status(path: &str) -> Result<()> {
    let project = config::load_project(std::path::Path::new(path))?;
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

fn status_icon(status: &devlaunch_core::model::ServiceStatus) -> &'static str {
    use devlaunch_core::model::ServiceStatus;
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
