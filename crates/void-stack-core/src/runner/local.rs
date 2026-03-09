use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::{info, warn};

use crate::error::{VoidStackError, Result};
use crate::model::{Service, ServiceState, ServiceStatus, Target};
use super::{Runner, StartResult};

/// Windows: CREATE_NO_WINDOW flag prevents cmd.exe from opening visible consoles.
/// Essential for GUI apps (Tauri) where child processes should be headless.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
                // Convert UNC path to Linux path for WSL execution
                let linux_dir = if is_wsl_unc_path(&working_dir) {
                    unc_to_linux_path(&working_dir)
                } else {
                    working_dir.clone()
                };

                let shell_cmd = format!(
                    "cd {} && {}",
                    shell_escape(&linux_dir),
                    service.command
                );

                let mut cmd = Command::new("wsl");

                // Use specific distro if we can detect it from the UNC path
                if let Some(distro) = unc_to_wsl_distro(&working_dir) {
                    cmd.args(["-d", &distro, "-e", "bash", "-c", &shell_cmd]);
                } else {
                    cmd.args(["-e", "bash", "-c", &shell_cmd]);
                }
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

        // On Windows, hide console windows when running from a GUI app
        #[cfg(target_os = "windows")]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let child = cmd.spawn().map_err(|e| {
            VoidStackError::ProcessStartFailed(format!(
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
            let mut kill_cmd = Command::new("taskkill");
            kill_cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
            kill_cmd.creation_flags(CREATE_NO_WINDOW);
            let output = kill_cmd.output().await?;

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
            let mut list_cmd = Command::new("tasklist");
            list_cmd.args(["/FI", &format!("PID eq {}", pid), "/NH"]);
            list_cmd.creation_flags(CREATE_NO_WINDOW);
            let output = list_cmd.output().await?;
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

/// Convert a WSL UNC path (`\\wsl.localhost\Distro\home\user\...`) to a Linux path (`/home/user/...`).
/// Returns the original path if it's not a UNC WSL path.
pub fn unc_to_linux_path(path: &str) -> String {
    // Match \\wsl.localhost\<distro>\<rest> or \\wsl$\<distro>\<rest>
    let normalized = path.replace('/', "\\");
    if let Some(rest) = normalized
        .strip_prefix(r"\\wsl.localhost\")
        .or_else(|| normalized.strip_prefix(r"\\wsl$\"))
    {
        // Skip the distro name
        if let Some(pos) = rest.find('\\') {
            let linux = rest[pos..].replace('\\', "/");
            if linux.is_empty() { return "/".to_string(); }
            return linux;
        }
        return "/".to_string();
    }
    path.to_string()
}

/// Extract the WSL distro name from a UNC path (`\\wsl.localhost\Ubuntu\...` → `Ubuntu`).
/// Returns None if it's not a UNC WSL path.
pub fn unc_to_wsl_distro(path: &str) -> Option<String> {
    let normalized = path.replace('/', "\\");
    let rest = normalized
        .strip_prefix(r"\\wsl.localhost\")
        .or_else(|| normalized.strip_prefix(r"\\wsl$\"))?;
    let distro = rest.split('\\').next()?;
    if distro.is_empty() { return None; }
    Some(distro.to_string())
}

/// Check if a path is a WSL UNC path.
pub fn is_wsl_unc_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.starts_with(r"\\wsl.localhost\") || lower.starts_with(r"\\wsl$\")
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

    #[test]
    fn test_unc_to_linux_path() {
        assert_eq!(
            unc_to_linux_path(r"\\wsl.localhost\Ubuntu\home\user\project"),
            "/home/user/project"
        );
        assert_eq!(
            unc_to_linux_path(r"\\wsl$\Debian\opt\app"),
            "/opt/app"
        );
        assert_eq!(
            unc_to_linux_path(r"\\wsl.localhost\Ubuntu"),
            "/"
        );
        // Non-UNC path returned as-is
        assert_eq!(
            unc_to_linux_path(r"F:\workspace\project"),
            r"F:\workspace\project"
        );
        // Linux path returned as-is
        assert_eq!(
            unc_to_linux_path("/home/user/project"),
            "/home/user/project"
        );
    }

    #[test]
    fn test_unc_to_wsl_distro() {
        assert_eq!(
            unc_to_wsl_distro(r"\\wsl.localhost\Ubuntu\home\user"),
            Some("Ubuntu".to_string())
        );
        assert_eq!(
            unc_to_wsl_distro(r"\\wsl$\Debian\opt"),
            Some("Debian".to_string())
        );
        assert_eq!(
            unc_to_wsl_distro(r"F:\workspace"),
            None
        );
    }

    #[test]
    fn test_is_wsl_unc_path() {
        assert!(is_wsl_unc_path(r"\\wsl.localhost\Ubuntu\home"));
        assert!(is_wsl_unc_path(r"\\wsl$\Debian\opt"));
        assert!(!is_wsl_unc_path(r"F:\workspace"));
        assert!(!is_wsl_unc_path("/home/user"));
    }
}
