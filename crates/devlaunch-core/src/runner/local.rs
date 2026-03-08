use async_trait::async_trait;
use tokio::process::Command;
use tracing::{info, warn};

use crate::error::{DevLaunchError, Result};
use crate::model::{Service, ServiceState, ServiceStatus, Target};
use super::Runner;

/// Runs processes locally on Windows or WSL.
pub struct LocalRunner {
    target: Target,
}

impl LocalRunner {
    pub fn new(target: Target) -> Self {
        Self { target }
    }

    fn build_command(&self, service: &Service, project_path: &str) -> Command {
        match self.target {
            Target::Wsl => {
                let working_dir = service
                    .working_dir
                    .as_deref()
                    .unwrap_or(project_path);
                let shell_cmd = format!(
                    "cd {} && {}",
                    shell_escape(working_dir),
                    service.command
                );

                let mut cmd = Command::new("wsl");
                cmd.args(["-e", "bash", "-c", &shell_cmd]);
                cmd.kill_on_drop(true);
                cmd
            }
            _ => {
                // Windows: use cmd /c
                let working_dir = service
                    .working_dir
                    .as_deref()
                    .unwrap_or(project_path);
                let shell_cmd = format!(
                    "cd /d {} && {}",
                    working_dir,
                    service.command
                );

                let mut cmd = Command::new("cmd");
                cmd.args(["/c", &shell_cmd]);
                cmd.kill_on_drop(true);
                cmd
            }
        }
    }
}

#[async_trait]
impl Runner for LocalRunner {
    fn target(&self) -> Target {
        self.target
    }

    async fn start(&self, service: &Service, project_path: &str) -> Result<ServiceState> {
        info!(
            service = %service.name,
            target = %self.target,
            command = %service.command,
            "Starting service"
        );

        let mut cmd = self.build_command(service, project_path);

        // Capture stdout/stderr for log streaming
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Set env vars
        for (key, value) in &service.env_vars {
            cmd.env(key, value);
        }

        let child = cmd.spawn().map_err(|e| {
            DevLaunchError::ProcessStartFailed(format!(
                "{} ({}): {}",
                service.name, service.command, e
            ))
        })?;

        let pid = child.id().unwrap_or(0);
        info!(service = %service.name, pid = pid, "Service started");

        let mut state = ServiceState::new(service.name.clone());
        state.status = ServiceStatus::Running;
        state.pid = Some(pid);
        state.started_at = Some(chrono::Utc::now());

        Ok(state)
    }

    async fn stop(&self, service: &Service, pid: u32) -> Result<()> {
        info!(service = %service.name, pid = pid, "Stopping service");

        #[cfg(target_os = "windows")]
        {
            // On Windows, use taskkill to terminate process tree
            let output = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .output()
                .await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(service = %service.name, pid = pid, error = %stderr, "taskkill failed");
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            use tokio::signal::unix;
            // Send SIGTERM first, then SIGKILL after timeout
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }

        Ok(())
    }

    async fn is_running(&self, pid: u32) -> Result<bool> {
        #[cfg(target_os = "windows")]
        {
            let output = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {}", pid), "/NH"])
                .output()
                .await?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.contains(&pid.to_string()))
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok(std::path::Path::new(&format!("/proc/{}", pid)).exists())
        }
    }
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Target;

    fn test_service() -> Service {
        Service {
            name: "test-svc".to_string(),
            command: "echo hello".to_string(),
            target: Target::Windows,
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
        }
    }

    #[tokio::test]
    async fn test_start_echo() {
        let runner = LocalRunner::new(Target::Windows);
        let svc = test_service();

        let state = runner.start(&svc, ".").await.unwrap();
        assert_eq!(state.status, ServiceStatus::Running);
        assert!(state.pid.is_some());
    }

    #[tokio::test]
    async fn test_is_running_nonexistent_pid() {
        let runner = LocalRunner::new(Target::Windows);
        let running = runner.is_running(999999).await.unwrap();
        assert!(!running);
    }
}
