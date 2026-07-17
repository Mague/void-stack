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
    let base = dirs::data_local_dir().context("Cannot determine local data directory")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_process_alive_self_and_dead() {
        // Our own process is alive.
        assert!(is_process_alive(std::process::id()));
        // u32::MAX is not a real PID on any supported platform.
        assert!(!is_process_alive(u32::MAX));
    }

    /// Exercises write_pid_file / read_pid_file / remove_pid_file against the
    /// real data dir. `data_dir()` is resolved from `dirs::data_local_dir()`
    /// with no injection point, so it cannot be redirected to a tempdir on
    /// Windows. To stay safe this test backs up any pre-existing PID file,
    /// refuses to touch it while a real daemon is alive, and restores the
    /// original state on the way out. All file mutation lives in this single
    /// test so nothing races it inside the crate's test binary.
    #[test]
    fn test_pid_file_write_read_remove() {
        let existing = read_pid_file().ok().flatten();
        if let Some(info) = &existing
            && is_process_alive(info.pid)
        {
            // A real daemon is running — never disturb its PID file. Still
            // assert the read path parsed it into a sane record.
            assert!(info.port > 0);
            return;
        }

        // Valid round-trip: every field survives write → read.
        let info = DaemonInfo {
            pid: std::process::id(),
            port: 50099,
            project_path: "C:/tmp/pid-fixture".to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
        };
        write_pid_file(&info).unwrap();
        let read = read_pid_file().unwrap().expect("PID file should exist");
        assert_eq!(read.pid, info.pid);
        assert_eq!(read.port, 50099);
        assert_eq!(read.project_path, "C:/tmp/pid-fixture");
        assert_eq!(read.started_at, "2026-01-01T00:00:00Z");

        // pid == 0 is the "no daemon" sentinel → read returns None.
        write_pid_file(&DaemonInfo {
            pid: 0,
            port: 50099,
            project_path: String::new(),
            started_at: String::new(),
        })
        .unwrap();
        assert!(read_pid_file().unwrap().is_none());

        // port == 0 is likewise treated as absent.
        write_pid_file(&DaemonInfo {
            pid: 1234,
            port: 0,
            project_path: String::new(),
            started_at: String::new(),
        })
        .unwrap();
        assert!(read_pid_file().unwrap().is_none());

        // remove clears the file; reading a missing file yields None, and a
        // second remove is a no-op.
        remove_pid_file().unwrap();
        assert!(read_pid_file().unwrap().is_none());
        remove_pid_file().unwrap();

        // Restore any prior (stale) file so we leave the dir as we found it.
        if let Some(prev) = existing {
            write_pid_file(&prev).unwrap();
        }
    }
}
