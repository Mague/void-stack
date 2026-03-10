use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

const PID_FILE_NAME: &str = "daemon.pid";
const APP_DIR_NAME: &str = "void-stack";

/// Information stored in the daemon PID file.
#[derive(Debug, Clone)]
pub struct DaemonInfo {
    pub pid: u32,
    pub port: u16,
    pub project_path: String,
    pub started_at: String,
}

/// Get the directory for VoidStack data files.
fn data_dir() -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .context("Cannot determine local data directory")?;
    let dir = base.join(APP_DIR_NAME);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// Full path to the PID file.
fn pid_file_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(PID_FILE_NAME))
}

/// Write daemon info to the PID file.
pub fn write_pid_file(info: &DaemonInfo) -> Result<()> {
    let path = pid_file_path()?;
    let content = format!(
        "pid={}\nport={}\nproject_path={}\nstarted_at={}",
        info.pid, info.port, info.project_path, info.started_at
    );
    fs::write(&path, content)?;
    tracing::info!(?path, "PID file written");
    Ok(())
}

/// Read daemon info from the PID file.
pub fn read_pid_file() -> Result<Option<DaemonInfo>> {
    let path = pid_file_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)?;
    let mut pid = 0u32;
    let mut port = 0u16;
    let mut project_path = String::new();
    let mut started_at = String::new();

    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "pid" => pid = value.trim().parse().unwrap_or(0),
                "port" => port = value.trim().parse().unwrap_or(0),
                "project_path" => project_path = value.trim().to_string(),
                "started_at" => started_at = value.trim().to_string(),
                _ => {}
            }
        }
    }

    if pid == 0 || port == 0 {
        return Ok(None);
    }

    Ok(Some(DaemonInfo {
        pid,
        port,
        project_path,
        started_at,
    }))
}

/// Remove the PID file.
pub fn remove_pid_file() -> Result<()> {
    let path = pid_file_path()?;
    if path.exists() {
        fs::remove_file(&path)?;
        tracing::info!(?path, "PID file removed");
    }
    Ok(())
}

/// Check if a process with the given PID is still running.
pub fn is_process_alive(pid: u32) -> bool {
    void_stack_core::process_util::is_pid_alive_sync(pid)
}
