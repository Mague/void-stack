mod lifecycle;
mod server;

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tokio::sync::broadcast;
use tonic::transport::Server;
use tracing::{error, info};

use void_stack_core::config;
use void_stack_core::manager::ProcessManager;
use void_stack_proto::pb;
use void_stack_proto::VoidStackServer;

use crate::lifecycle::{DaemonInfo, is_process_alive, read_pid_file, remove_pid_file, write_pid_file};
use crate::server::VoidStackService;

const DEFAULT_PORT: u16 = 50051;

#[derive(Parser)]
#[command(name = "void-daemon", about = "VoidStack background daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon for a project
    Start {
        /// Path to project directory
        #[arg(short, long, default_value = ".")]
        path: String,

        /// gRPC listen port
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Stop a running daemon
    Stop,
    /// Check daemon status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "void_stack=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start { path, port } => cmd_start(&path, port).await,
        Commands::Stop => cmd_stop().await,
        Commands::Status => cmd_status().await,
    }
}

async fn cmd_start(path: &str, port: u16) -> Result<()> {
    // Check if daemon is already running
    if let Some(info) = read_pid_file()? {
        if is_process_alive(info.pid) {
            bail!(
                "Daemon already running (PID: {}, port: {}, project: {})",
                info.pid,
                info.port,
                info.project_path
            );
        }
        // Stale PID file, remove it
        remove_pid_file()?;
    }

    // Load project config
    let project = config::load_project(Path::new(path))
        .context("Failed to load void-stack.toml — run 'void init' first")?;

    info!(project = %project.name, port, "Starting daemon");

    // Write PID file
    let daemon_info = DaemonInfo {
        pid: std::process::id(),
        port,
        project_path: std::path::Path::new(path)
            .canonicalize()
            .unwrap_or_else(|_| path.into())
            .to_string_lossy()
            .to_string(),
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    write_pid_file(&daemon_info)?;

    // Create ProcessManager and broadcast channel for logs
    let manager = Arc::new(ProcessManager::new(project));
    let (log_tx, _) = broadcast::channel::<pb::LogEntry>(1024);

    // Build gRPC server
    let addr = format!("127.0.0.1:{port}").parse()?;
    let service = VoidStackService::new(manager.clone(), log_tx);

    info!(%addr, "gRPC server listening");

    // Graceful shutdown on Ctrl+C
    let shutdown_manager = manager.clone();
    Server::builder()
        .add_service(VoidStackServer::new(service))
        .serve_with_shutdown(addr, async {
            tokio::signal::ctrl_c().await.ok();
            info!("Shutdown signal received");

            // Stop all services before exiting
            if let Err(e) = shutdown_manager.stop_all().await {
                error!(error = %e, "Error stopping services during shutdown");
            }

            // Remove PID file
            if let Err(e) = remove_pid_file() {
                error!(error = %e, "Error removing PID file");
            }
        })
        .await?;

    info!("Daemon stopped");
    Ok(())
}

async fn cmd_stop() -> Result<()> {
    let info = read_pid_file()?
        .context("No daemon is running (PID file not found)")?;

    if !is_process_alive(info.pid) {
        remove_pid_file()?;
        println!("Daemon was not running (stale PID file removed)");
        return Ok(());
    }

    // Try graceful shutdown via gRPC
    let addr = format!("http://127.0.0.1:{}", info.port);
    match void_stack_proto::VoidStackClient::connect(addr).await {
        Ok(mut client) => {
            println!("Sending shutdown to daemon (PID: {})...", info.pid);
            client
                .shutdown(pb::ShutdownRequest {})
                .await
                .context("Shutdown RPC failed")?;
            println!("Daemon shutdown initiated");
        }
        Err(_) => {
            // Fallback: kill the process directly
            println!("Cannot connect to daemon, killing process {}...", info.pid);
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("taskkill")
                    .args(["/PID", &info.pid.to_string(), "/T", "/F"])
                    .output()?;
            }
            #[cfg(not(target_os = "windows"))]
            {
                unsafe {
                    libc::kill(info.pid as i32, libc::SIGTERM);
                }
            }
        }
    }

    remove_pid_file()?;
    println!("Daemon stopped");
    Ok(())
}

async fn cmd_status() -> Result<()> {
    match read_pid_file()? {
        None => {
            println!("No daemon is running");
        }
        Some(info) => {
            if !is_process_alive(info.pid) {
                remove_pid_file()?;
                println!("No daemon is running (stale PID file cleaned up)");
                return Ok(());
            }

            // Try to get live info via gRPC
            let addr = format!("http://127.0.0.1:{}", info.port);
            match void_stack_proto::VoidStackClient::connect(addr).await {
                Ok(mut client) => {
                    let resp = client
                        .ping(pb::PingRequest {})
                        .await?
                        .into_inner();

                    println!("VoidStack Daemon v{}", resp.version);
                    println!("  PID:       {}", info.pid);
                    println!("  Port:      {}", info.port);
                    println!("  Project:   {}", resp.project_name);
                    println!("  Uptime:    {}s", resp.uptime_secs);
                    println!("  Services:  {}/{} running", resp.services_running, resp.services_total);
                    println!("  Started:   {}", info.started_at);
                }
                Err(_) => {
                    println!("Daemon is running but not responding to gRPC");
                    println!("  PID:     {}", info.pid);
                    println!("  Port:    {}", info.port);
                    println!("  Project: {}", info.project_path);
                }
            }
        }
    }
    Ok(())
}
