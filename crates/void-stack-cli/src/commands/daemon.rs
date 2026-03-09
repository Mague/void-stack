use std::time::Duration;

use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_proto::client::DaemonClient;

use crate::DEFAULT_DAEMON_PORT;

pub async fn cmd_daemon_start(project_name: &str, port: u16) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    println!("To start the daemon, run:");
    println!(
        "  void-daemon start -p \"{}\" --port {}",
        project.path, port
    );
    Ok(())
}

pub async fn cmd_daemon_stop() -> Result<()> {
    let addr = format!("http://127.0.0.1:{}", DEFAULT_DAEMON_PORT);
    match DaemonClient::connect_with_timeout(&addr, Duration::from_secs(3)).await {
        Ok(mut client) => {
            client.shutdown().await?;
            println!("Daemon shutdown initiated.");
        }
        Err(_) => {
            println!(
                "No daemon is running (cannot connect on port {}).",
                DEFAULT_DAEMON_PORT
            );
        }
    }
    Ok(())
}

pub async fn cmd_daemon_status() -> Result<()> {
    let addr = format!("http://127.0.0.1:{}", DEFAULT_DAEMON_PORT);
    match DaemonClient::connect_with_timeout(&addr, Duration::from_secs(3)).await {
        Ok(mut client) => {
            let info = client.ping().await?;
            println!("VoidStack Daemon v{}", info.version);
            println!("  Project:  {}", info.project_name);
            println!("  Uptime:   {}s", info.uptime_secs);
            println!(
                "  Services: {}/{} running",
                info.services_running, info.services_total
            );
        }
        Err(_) => {
            println!(
                "No daemon is running (cannot connect on port {}).",
                DEFAULT_DAEMON_PORT
            );
        }
    }
    Ok(())
}
