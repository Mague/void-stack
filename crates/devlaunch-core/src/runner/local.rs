use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::{info, warn};

use crate::error::{DevLaunchError, Result};
use crate::model::{Service, ServiceState, ServiceStatus, Target};
use super::{Runner, StartResult};

/// Runs processes locally on Windows or WSL.
pub struct LocalRunner {
    target: Target,
}

impl LocalRunner {
    pub fn new(target: Target) -> Self {
        Self { target }
    }

    fn build_command(&self, service: &Service, project_path: &str) -> Command {
        let working_dir = strip_win_prefix(
            service.working_dir.as_deref().unwrap_or(project_path),
        );

        match self.target {
            Target::Wsl => {
                let shell_cmd = format!(
                    "cd {} && {}",
                    shell_escape(&working_dir),
                    service.command
                );

                let mut cmd = Command::new("wsl");
                cmd.args(["-e", "bash", "-c", &shell_cmd]);
                cmd
            }
            _ => {
                // Windows: resolve python to virtualenv if available,
                // then run via cmd /c call (call keeps pipes alive for
                // batch files and works for .exe too).
                let resolved = resolve_python_venv(&service.command, &working_dir);

                let mut cmd = Command::new("cmd");
                cmd.args(["/c", &format!("call {}", resolved)]);
                cmd.current_dir(&working_dir);
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

    async fn start(&self, service: &Service, project_path: &str) -> Result<StartResult> {
        info!(
            service = %service.name,
            target = %self.target,
            command = %service.command,
            "Starting service"
        );

        let mut cmd = self.build_command(service, project_path);

        // Close stdin to prevent child processes from blocking on stdin reads.
        // Node.js/npm can deadlock on Windows when stdin is inherited and piped.
        cmd.stdin(std::process::Stdio::null());

        // Capture stdout/stderr for log streaming
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Set env vars
        for (key, value) in &service.env_vars {
            cmd.env(key, value);
        }

        // Force unbuffered output for Python so logs arrive in real-time
        cmd.env("PYTHONUNBUFFERED", "1");
        // Force color output so tools like Vite still print URLs when piped
        cmd.env("FORCE_COLOR", "1");

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

        Ok(StartResult { state, child })
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
            // Send SIGTERM
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

/// Strip the `\\?\` extended-length path prefix that Rust's canonicalize()
/// adds on Windows. Many programs (Node.js, Python, etc.) don't handle it.
pub fn strip_win_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}


/// Auto-detect virtualenvs and resolve python/pip commands to the venv
/// executable. Searches the working directory and its parent (for monorepos).
fn resolve_python_venv(command: &str, working_dir: &str) -> String {
    let parts: Vec<&str> = command.splitn(2, char::is_whitespace).collect();
    let program = parts[0];
    let rest = parts.get(1).copied().unwrap_or("");

    let python_cmds = ["python", "python3", "python3.exe", "python.exe"];
    let venv_cmds = ["pip", "pip3", "pytest", "uvicorn", "gunicorn", "flask",
                      "django-admin", "celery", "alembic", "mypy", "ruff", "black", "isort"];

    let is_python = python_cmds.iter().any(|p| program.eq_ignore_ascii_case(p));
    let is_venv_tool = venv_cmds.iter().any(|p| program.eq_ignore_ascii_case(p));

    if !is_python && !is_venv_tool {
        return command.to_string();
    }

    let dir = Path::new(working_dir);
    let venv_dirs = ["venv", ".venv", "env", ".env"];

    // Search working_dir and ancestors (up to 4 levels) for monorepos
    // e.g., project/.venv/ should be found from project/gui/backend/
    let search_dirs: Vec<&Path> = {
        let mut dirs = vec![dir];
        let mut current = dir;
        for _ in 0..4 {
            match current.parent() {
                Some(parent) if parent != current => {
                    dirs.push(parent);
                    current = parent;
                }
                _ => break,
            }
        }
        dirs
    };

    for search_dir in &search_dirs {
        for venv in &venv_dirs {
            let scripts = search_dir.join(venv).join("Scripts"); // Windows
            if !scripts.exists() {
                continue;
            }

            let exe_name = if is_python {
                "python.exe".to_string()
            } else {
                format!("{}.exe", program)
            };

            let exe = scripts.join(&exe_name);
            if exe.exists() {
                let exe_path = strip_win_prefix(&exe.to_string_lossy());
                let location = if *search_dir == dir { "local" } else { "ancestor" };
                info!(
                    venv = %venv,
                    path = %exe_path,
                    location = %location,
                    "Auto-detected virtualenv"
                );
                // Return the full path to the venv executable + original args
                if rest.is_empty() {
                    return exe_path;
                }
                return format!("{} {}", exe_path, rest);
            }
        }
    }

    // No venv found, return original command
    command.to_string()
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

        let result = runner.start(&svc, ".").await.unwrap();
        assert_eq!(result.state.status, ServiceStatus::Running);
        assert!(result.state.pid.is_some());
    }

    #[tokio::test]
    async fn test_is_running_nonexistent_pid() {
        let runner = LocalRunner::new(Target::Windows);
        let running = runner.is_running(999999).await.unwrap();
        assert!(!running);
    }

    #[test]
    fn test_strip_win_prefix() {
        assert_eq!(strip_win_prefix(r"\\?\F:\workspace"), r"F:\workspace");
        assert_eq!(strip_win_prefix(r"F:\workspace"), r"F:\workspace");
    }

    #[test]
    fn test_resolve_python_no_venv() {
        // When no venv exists, returns original command
        assert_eq!(resolve_python_venv("python main.py", "."), "python main.py");
    }
}
