use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::{info, warn};

use super::{Runner, StartResult};
use crate::error::{Result, VoidStackError};
use crate::model::{Service, ServiceState, ServiceStatus, Target};
use crate::process_util::HideWindow;

/// Runs processes locally on Windows or WSL.
///
/// # Trust model
/// Service `command` strings are executed verbatim through the platform
/// shell. Project configs (`void-stack.toml`, registered services) are
/// trusted input — the launchers require a one-time confirmation before
/// running services from a newly registered config
/// (see `config::is_project_trusted`).
pub struct LocalRunner {
    target: Target,
}

impl LocalRunner {
    pub fn new(target: Target) -> Self {
        Self { target }
    }

    fn build_command(&self, service: &Service, project_path: &str) -> Command {
        let working_dir = strip_win_prefix(service.working_dir.as_deref().unwrap_or(project_path));

        match self.target {
            Target::Wsl => {
                // Convert UNC path to Linux path for WSL execution
                let linux_dir = if is_wsl_unc_path(&working_dir) {
                    unc_to_linux_path(&working_dir)
                } else {
                    working_dir.clone()
                };

                // Trim accidental leading/trailing whitespace and rewrite the
                // package manager when the registered command says `npm run`
                // but the WSL project actually uses pnpm/yarn.
                let trimmed = service.command.trim();
                let final_cmd = maybe_replace_pkg_manager(trimmed, &working_dir);

                let shell_cmd = format!("cd {} && {}", shell_escape(&linux_dir), final_cmd);

                let mut cmd = Command::new("wsl");

                // Use a login + interactive shell (`bash -l -i -c`) so
                // BOTH `~/.profile`/`~/.bash_profile` AND `~/.bashrc`
                // run. asdf, nvm, rbenv and pyenv init scripts live in
                // `.bashrc` on Ubuntu defaults; that file early-returns
                // when not interactive (`case $- in *i*) ;; *) return;;`),
                // so `-l` alone leaves PATH without the shim
                // directories and `mix` / `elixir` / pinned `node`
                // come back as "command not found". `-i` flips the
                // guard. Stdin is `null`'d on spawn so the shell can
                // never block on a prompt.
                if let Some(distro) = unc_to_wsl_distro(&working_dir) {
                    cmd.args(["-d", &distro, "-e", "bash", "-l", "-i", "-c", &shell_cmd]);
                } else {
                    cmd.args(["-e", "bash", "-l", "-i", "-c", &shell_cmd]);
                }
                cmd
            }
            _ => {
                // Resolve python to virtualenv if available
                let resolved = resolve_python_venv(&service.command, &working_dir);

                // Windows: cmd /c <command>. Two rules:
                // 1. If `resolved` is itself a shell wrapper (starts with
                //    `cmd` or `powershell`), pass it through verbatim —
                //    wrapping a shell with another `call` is wrong.
                // 2. If it's an absolute venv exe path (`…\.venv\…\foo.exe
                //    args`), drop the `call` prefix and let cmd /c invoke
                //    the exe directly. Rust's process::Command re-quotes
                //    the inner string when constructing the Windows
                //    command line, so `call "exe" args` becomes
                //    `\"exe\"` and cmd.exe reads it as a single literal
                //    filename (with quotes), failing with "no se reconoce".
                //    `cmd /c exe args` (no `call`, no quotes) works.
                // 3. Otherwise fall back to `call <resolved>` for plain
                //    commands so pipes stay alive for batch files.
                #[cfg(target_os = "windows")]
                {
                    let mut cmd = Command::new("cmd");
                    let trimmed = resolved.trim_start();
                    let is_shell_wrapper = trimmed.starts_with("cmd ")
                        || trimmed.starts_with("cmd.exe")
                        || trimmed.starts_with("cmd /")
                        || trimmed.starts_with("powershell")
                        || trimmed.starts_with("pwsh");
                    let call_arg = if is_shell_wrapper {
                        resolved.clone()
                    } else if resolved.contains(".venv") || resolved.contains(".env") {
                        // Absolute venv exe path — drop `call` and quotes
                        // entirely so process::Command doesn't double-escape.
                        resolved.clone()
                    } else {
                        format!("call {}", resolved)
                    };
                    cmd.args(["/c", &call_arg]);
                    cmd.current_dir(&working_dir);
                    cmd
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let mut cmd = crate::process_util::shell_command(&resolved);
                    cmd.current_dir(&working_dir);
                    cmd
                }
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

        // Hide console windows when running from a GUI app
        cmd.hide_window();

        // On Unix, place the service in its own process group so stop() can
        // signal the entire tree (`npm run dev` → node → workers). Killing
        // only the direct child would orphan its descendants.
        #[cfg(unix)]
        cmd.process_group(0);

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
            kill_cmd.hide_window();
            let output = kill_cmd.output().await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(service = %service.name, pid = pid, error = %stderr, "taskkill failed");
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            stop_unix_process_group(&service.name, pid).await;
        }

        Ok(())
    }

    async fn is_running(&self, pid: u32) -> Result<bool> {
        Ok(crate::process_util::is_pid_alive_async(pid).await)
    }
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Terminate a service's whole process tree on Unix.
///
/// Services are spawned in their own process group (see `start`), so the
/// group id equals the child's PID. SIGTERM is sent to the group first; if
/// any member is still alive after a ~3s grace period, the group gets
/// SIGKILL. Falls back to signaling the single PID when the group signal
/// fails (e.g. a process adopted after a daemon restart that is not a
/// group leader).
#[cfg(not(target_os = "windows"))]
async fn stop_unix_process_group(service_name: &str, pid: u32) {
    use std::time::Duration;

    const GRACE: Duration = Duration::from_secs(3);
    const POLL: Duration = Duration::from_millis(100);

    let pgid = -(pid as i32);
    let group_signal_ok = unsafe { libc::kill(pgid, libc::SIGTERM) } == 0;
    if !group_signal_ok {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }

    let deadline = tokio::time::Instant::now() + GRACE;
    while tokio::time::Instant::now() < deadline {
        let alive = if group_signal_ok {
            unsafe { libc::kill(pgid, 0) == 0 }
        } else {
            unsafe { libc::kill(pid as i32, 0) == 0 }
        };
        if !alive {
            return;
        }
        tokio::time::sleep(POLL).await;
    }

    warn!(
        service = %service_name,
        pid = pid,
        "Process group still alive after grace period — escalating to SIGKILL"
    );
    unsafe {
        if group_signal_ok {
            libc::kill(pgid, libc::SIGKILL);
        } else {
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
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
            if linux.is_empty() {
                return "/".to_string();
            }
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
    if distro.is_empty() {
        return None;
    }
    Some(distro.to_string())
}

/// Check if a path is a WSL UNC path.
pub fn is_wsl_unc_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.starts_with(r"\\wsl.localhost\") || lower.starts_with(r"\\wsl$\")
}

/// Peel common shell wrappers (`cmd /c …`, `powershell -Command "…"`) so the
/// venv resolver can find the real program. Returns the substring that
/// contains the actual command; callers re-stitch the wrapper afterward.
pub(crate) fn strip_shell_wrapper(command: &str) -> &str {
    let cmd = command.trim();

    if let Some(rest) = cmd
        .strip_prefix("cmd /c ")
        .or_else(|| cmd.strip_prefix("cmd /C "))
    {
        // `cmd /c chcp 65001 && uvicorn …` — the real command is the last
        // segment after &&. If there's no &&, the whole tail is the command.
        if let Some(last) = rest.split("&&").last() {
            return last.trim();
        }
        return rest.trim();
    }

    if cmd.to_lowercase().contains("powershell")
        && let Some(idx) = cmd.find('"')
    {
        return cmd[idx + 1..].trim_end_matches('"');
    }

    cmd
}

/// If the registered command says `npm run …` but the WSL project actually
/// uses pnpm or yarn, rewrite the leading `npm`. Looks at the working dir
/// for `pnpm-lock.yaml` / `yarn.lock`. Anything else is returned unchanged.
pub(crate) fn maybe_replace_pkg_manager(command: &str, working_dir: &str) -> String {
    if !command.trim_start().starts_with("npm run") {
        return command.to_string();
    }
    let dir = std::path::Path::new(working_dir);
    if dir.join("pnpm-lock.yaml").exists() {
        return command.replacen("npm", "pnpm", 1);
    }
    if dir.join("yarn.lock").exists() {
        return command.replacen("npm", "yarn", 1);
    }
    command.to_string()
}

/// Auto-detect virtualenvs and resolve python/pip commands to the venv
/// executable. Searches the working directory and its parent (for monorepos).
fn resolve_python_venv(command: &str, working_dir: &str) -> String {
    // Peel `cmd /c chcp 65001 && uvicorn …` so the program lookup sees
    // `uvicorn`, not `cmd`. The wrapper is preserved when we splice the
    // resolved venv path back into the original command string.
    let effective = strip_shell_wrapper(command);
    let parts: Vec<&str> = effective.splitn(2, char::is_whitespace).collect();
    let program = parts[0];
    let rest = parts.get(1).copied().unwrap_or("");

    let python_cmds = ["python", "python3", "python3.exe", "python.exe"];
    let venv_cmds = [
        "pip",
        "pip3",
        "pytest",
        "uvicorn",
        "gunicorn",
        "flask",
        "django-admin",
        "celery",
        "alembic",
        "mypy",
        "ruff",
        "black",
        "isort",
    ];

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

    // On Windows venvs use Scripts/ with .exe; on Unix they use bin/ without extension
    #[cfg(target_os = "windows")]
    let venv_bin_dirs: &[&str] = &["Scripts"];
    #[cfg(not(target_os = "windows"))]
    let venv_bin_dirs: &[&str] = &["bin"];

    for search_dir in &search_dirs {
        for venv in &venv_dirs {
            for bin_dir in venv_bin_dirs {
                let scripts = search_dir.join(venv).join(bin_dir);
                if !scripts.exists() {
                    continue;
                }

                #[cfg(target_os = "windows")]
                let exe_name = if is_python {
                    "python.exe".to_string()
                } else {
                    format!("{}.exe", program)
                };
                #[cfg(not(target_os = "windows"))]
                let exe_name = if is_python {
                    "python3".to_string()
                } else {
                    program.to_string()
                };

                let exe = scripts.join(&exe_name);
                if exe.exists() {
                    let exe_path = strip_win_prefix(&exe.to_string_lossy());
                    let location = if *search_dir == dir {
                        "local"
                    } else {
                        "ancestor"
                    };
                    info!(
                        venv = %venv,
                        path = %exe_path,
                        location = %location,
                        "Auto-detected virtualenv"
                    );
                    // If the caller wrapped the command (`cmd /c … && uvicorn`),
                    // keep the wrapper and only swap the program token. Plain
                    // commands just get `<exe> <args>`.
                    if command != effective {
                        return command.replacen(program, &exe_path, 1);
                    }
                    if rest.is_empty() {
                        return exe_path;
                    }
                    return format!("{} {}", exe_path, rest);
                }
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
            docker: None,
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
        assert_eq!(unc_to_linux_path(r"\\wsl$\Debian\opt\app"), "/opt/app");
        assert_eq!(unc_to_linux_path(r"\\wsl.localhost\Ubuntu"), "/");
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
        assert_eq!(unc_to_wsl_distro(r"F:\workspace"), None);
    }

    #[test]
    fn test_is_wsl_unc_path() {
        assert!(is_wsl_unc_path(r"\\wsl.localhost\Ubuntu\home"));
        assert!(is_wsl_unc_path(r"\\wsl$\Debian\opt"));
        assert!(!is_wsl_unc_path(r"F:\workspace"));
        assert!(!is_wsl_unc_path("/home/user"));
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("hello"), "'hello'");
        assert_eq!(shell_escape("/path/to/dir"), "'/path/to/dir'");
        assert_eq!(shell_escape("it's a test"), "'it'\\''s a test'");
    }

    #[test]
    fn test_resolve_python_venv_non_python_command() {
        assert_eq!(resolve_python_venv("npm run dev", "."), "npm run dev");
        assert_eq!(resolve_python_venv("cargo run", "."), "cargo run");
        assert_eq!(resolve_python_venv("node server.js", "."), "node server.js");
    }

    #[test]
    fn test_resolve_python_venv_with_venv() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir
            .path()
            .join(".venv")
            .join(if cfg!(target_os = "windows") {
                "Scripts"
            } else {
                "bin"
            });
        std::fs::create_dir_all(&scripts).unwrap();
        let exe_name = if cfg!(target_os = "windows") {
            "python.exe"
        } else {
            "python3"
        };
        std::fs::write(scripts.join(exe_name), "").unwrap();

        let working_dir = dir.path().to_string_lossy().to_string();
        let result = resolve_python_venv("python main.py", &working_dir);
        assert!(
            result.contains(".venv"),
            "Should resolve to venv python: {}",
            result
        );
        assert!(result.contains("main.py"), "Should keep args: {}", result);
    }

    #[test]
    fn test_resolve_python_venv_pip() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir
            .path()
            .join(".venv")
            .join(if cfg!(target_os = "windows") {
                "Scripts"
            } else {
                "bin"
            });
        std::fs::create_dir_all(&scripts).unwrap();
        let exe_name = if cfg!(target_os = "windows") {
            "pip.exe"
        } else {
            "pip"
        };
        std::fs::write(scripts.join(exe_name), "").unwrap();

        let working_dir = dir.path().to_string_lossy().to_string();
        let result = resolve_python_venv("pip install flask", &working_dir);
        assert!(
            result.contains(".venv"),
            "Should resolve to venv pip: {}",
            result
        );
    }

    #[test]
    fn test_resolve_python_venv_no_args() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir
            .path()
            .join(".venv")
            .join(if cfg!(target_os = "windows") {
                "Scripts"
            } else {
                "bin"
            });
        std::fs::create_dir_all(&scripts).unwrap();
        let exe_name = if cfg!(target_os = "windows") {
            "python.exe"
        } else {
            "python3"
        };
        std::fs::write(scripts.join(exe_name), "").unwrap();

        let working_dir = dir.path().to_string_lossy().to_string();
        let result = resolve_python_venv("python", &working_dir);
        assert!(result.contains(".venv"), "Should resolve: {}", result);
        assert!(!result.contains(' '), "No args, no space: {}", result);
    }

    #[test]
    fn test_local_runner_target() {
        let runner = LocalRunner::new(Target::Windows);
        assert_eq!(runner.target(), Target::Windows);
        let runner2 = LocalRunner::new(Target::Wsl);
        assert_eq!(runner2.target(), Target::Wsl);
    }

    #[test]
    fn test_unc_to_linux_path_forward_slashes() {
        // Forward slashes should also work
        assert_eq!(
            unc_to_linux_path("//wsl.localhost/Ubuntu/home/user"),
            "/home/user"
        );
    }

    #[test]
    fn test_unc_to_wsl_distro_empty() {
        assert_eq!(unc_to_wsl_distro(r"\\wsl.localhost\"), None);
    }

    #[test]
    fn test_strip_shell_wrapper_cmd_c() {
        assert_eq!(
            strip_shell_wrapper("cmd /c chcp 65001 && uvicorn main:app"),
            "uvicorn main:app"
        );
        assert_eq!(
            strip_shell_wrapper("cmd /C uvicorn main:app --port 8000"),
            "uvicorn main:app --port 8000"
        );
        // No wrapper → returned trimmed.
        assert_eq!(
            strip_shell_wrapper("  uvicorn main:app  "),
            "uvicorn main:app"
        );
    }

    #[test]
    fn test_strip_shell_wrapper_powershell() {
        assert_eq!(
            strip_shell_wrapper(r#"powershell -NoProfile -Command "uvicorn main:app""#),
            "uvicorn main:app"
        );
    }

    #[test]
    fn test_resolve_venv_through_cmd_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir
            .path()
            .join(".venv")
            .join(if cfg!(target_os = "windows") {
                "Scripts"
            } else {
                "bin"
            });
        std::fs::create_dir_all(&scripts).unwrap();
        let exe_name = if cfg!(target_os = "windows") {
            "uvicorn.exe"
        } else {
            "uvicorn"
        };
        std::fs::write(scripts.join(exe_name), "").unwrap();

        let working_dir = dir.path().to_string_lossy().to_string();
        let original = "cmd /c chcp 65001 && uvicorn main:app --port 8000";
        let result = resolve_python_venv(original, &working_dir);

        // The wrapper must survive untouched; the program token (`uvicorn`) must
        // be swapped for the absolute venv path.
        assert!(
            result.starts_with("cmd /c chcp 65001 && "),
            "wrapper lost: {}",
            result
        );
        assert!(result.contains(".venv"), "venv not spliced: {}", result);
        assert!(
            result.ends_with("main:app --port 8000"),
            "args lost: {}",
            result
        );
    }

    #[test]
    fn test_maybe_replace_pkg_manager_pnpm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let working_dir = dir.path().to_string_lossy().to_string();
        assert_eq!(
            maybe_replace_pkg_manager("npm run dev", &working_dir),
            "pnpm run dev"
        );
    }

    #[test]
    fn test_maybe_replace_pkg_manager_yarn() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let working_dir = dir.path().to_string_lossy().to_string();
        assert_eq!(
            maybe_replace_pkg_manager("npm run build", &working_dir),
            "yarn run build"
        );
    }

    #[test]
    fn test_maybe_replace_pkg_manager_no_lockfile_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let working_dir = dir.path().to_string_lossy().to_string();
        assert_eq!(
            maybe_replace_pkg_manager("npm run dev", &working_dir),
            "npm run dev"
        );
    }

    #[test]
    fn test_maybe_replace_pkg_manager_ignores_non_npm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let working_dir = dir.path().to_string_lossy().to_string();
        // Doesn't start with "npm run" → returned as-is.
        assert_eq!(
            maybe_replace_pkg_manager("pnpm dev:admin", &working_dir),
            "pnpm dev:admin"
        );
    }

    #[test]
    fn test_trim_leading_space_command() {
        // financiApp regression: " pnpm dev:admin" trimmed correctly.
        assert_eq!(" pnpm dev:admin".trim(), "pnpm dev:admin");
    }

    /// Reach into the assembled `Command`'s argv to recover the `/c` arg
    /// without spawning a process. `Command::get_args` returns an
    /// `OsStr` iterator; on Windows the args were inserted as plain
    /// `&str` so the lossy conversion round-trips cleanly.
    #[cfg(target_os = "windows")]
    fn extract_c_arg(cmd: &Command) -> String {
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        // Layout is `["/c", "<call_arg>"]`.
        assert_eq!(args.first().map(String::as_str), Some("/c"));
        args.get(1).cloned().unwrap_or_default()
    }

    #[cfg(target_os = "windows")]
    fn service_with_command(c: &str) -> Service {
        Service {
            name: "t".to_string(),
            command: c.to_string(),
            target: Target::Windows,
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
            docker: None,
        }
    }

    /// Spawning a service that forks children (like `npm run dev` → node)
    /// and stopping it must kill the WHOLE tree, not just the shell.
    /// The service runs `sleep 100 & sleep 100` under `sh -c`, producing
    /// two grandchildren; after stop() no member of the process group may
    /// survive.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_stop_kills_full_process_tree_unix() {
        use std::time::Duration;

        let runner = LocalRunner::new(Target::native());
        let mut svc = test_service();
        svc.command = "sleep 100 & sleep 100".to_string();

        let result = runner.start(&svc, ".").await.unwrap();
        let pid = result.state.pid.unwrap();
        let mut child = result.child;

        // Reap the direct child in the background, mirroring production
        // where spawn_log_reader owns child.wait(). Without this the child
        // would linger as a zombie and keep the group "alive".
        let reaper = tokio::spawn(async move {
            let _ = child.wait().await;
        });

        // Give sh time to fork the two sleeps.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // The group must exist before stop (group id == child pid).
        assert!(
            unsafe { libc::kill(-(pid as i32), 0) == 0 },
            "process group should be alive before stop"
        );

        runner.stop(&svc, pid).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(5), reaper).await;

        // Poll until every member of the group is gone (ESRCH).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let group_alive = unsafe { libc::kill(-(pid as i32), 0) == 0 };
            if !group_alive {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "descendant processes survived stop(): group {} still alive",
                pid
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_build_command_plain_uses_call_prefix() {
        let runner = LocalRunner::new(Target::Windows);
        let svc = service_with_command("npm run dev");
        let cmd = runner.build_command(&svc, ".");
        assert_eq!(extract_c_arg(&cmd), "call npm run dev");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_build_command_venv_path_skips_call_and_quotes() {
        // Regression: previously emitted `call "F:\…\.venv\…\uvicorn.exe" args`
        // which Rust's Command re-quoted to `\"…\"`, breaking cmd /c.
        // The fix: drop `call` AND the quotes — cmd /c handles bare
        // absolute exe paths fine even when the path contains dots.
        let runner = LocalRunner::new(Target::Windows);
        let svc = service_with_command(
            r"F:\workspace\proj\.venv\Scripts\uvicorn.exe main:app --port 8000",
        );
        let arg = extract_c_arg(&runner.build_command(&svc, "."));
        assert!(
            !arg.contains("call "),
            "should NOT prepend `call` for venv exe: {arg}"
        );
        assert!(
            !arg.contains('"'),
            "should NOT add quotes around the venv exe: {arg}"
        );
        assert!(arg.contains(".venv"), "must keep the venv path: {arg}");
        assert!(arg.ends_with("main:app --port 8000"), "args lost: {arg}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_build_command_shell_wrapper_passes_through() {
        // Service already wraps in cmd /c — we must NOT re-wrap with
        // `call cmd …` (which produces a literal cmd argument to call).
        let runner = LocalRunner::new(Target::Windows);
        let svc = service_with_command("cmd /c chcp 65001 && uvicorn main:app");
        let arg = extract_c_arg(&runner.build_command(&svc, "."));
        assert!(
            !arg.starts_with("call "),
            "shell wrappers must pass through verbatim: {arg}"
        );
        assert!(arg.starts_with("cmd /c "), "wrapper preserved: {arg}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_build_command_powershell_passes_through() {
        let runner = LocalRunner::new(Target::Windows);
        let svc = service_with_command(r#"powershell -NoProfile -Command "Get-Date""#);
        let arg = extract_c_arg(&runner.build_command(&svc, "."));
        assert!(
            !arg.starts_with("call "),
            "powershell wrappers must pass through verbatim: {arg}"
        );
        assert!(arg.starts_with("powershell"), "wrapper preserved: {arg}");
    }

    /// Recover the full argv of an assembled `Command` (platform-agnostic).
    fn cmd_args(cmd: &Command) -> Vec<String> {
        cmd.as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect()
    }

    fn wsl_service(command: &str) -> Service {
        Service {
            name: "wsl-svc".to_string(),
            command: command.to_string(),
            target: Target::Wsl,
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
            docker: None,
        }
    }

    /// WSL target with a plain Linux working dir: `bash -l -i -c 'cd … && cmd'`
    /// with no `-d <distro>` prefix (working dir is not a UNC path).
    #[test]
    fn test_build_command_wsl_local_dir() {
        let runner = LocalRunner::new(Target::Wsl);
        let svc = wsl_service("npm run dev");
        let cmd = runner.build_command(&svc, "/home/u/app");
        assert_eq!(cmd.as_std().get_program().to_string_lossy(), "wsl");
        let args = cmd_args(&cmd);
        assert_eq!(
            args,
            vec![
                "-e",
                "bash",
                "-l",
                "-i",
                "-c",
                "cd '/home/u/app' && npm run dev"
            ]
        );
    }

    /// WSL target with a `\\wsl.localhost\Distro\…` working dir: the UNC path
    /// is converted to a Linux path and the distro is passed via `-d`.
    #[test]
    fn test_build_command_wsl_unc_path_uses_distro() {
        let runner = LocalRunner::new(Target::Wsl);
        let mut svc = wsl_service("pnpm dev");
        svc.working_dir = Some(r"\\wsl.localhost\Ubuntu\home\u\app".to_string());
        let cmd = runner.build_command(&svc, ".");
        let args = cmd_args(&cmd);
        assert_eq!(&args[0..2], &["-d", "Ubuntu"]);
        assert_eq!(&args[2..7], &["-e", "bash", "-l", "-i", "-c"]);
        assert_eq!(args[7], "cd '/home/u/app' && pnpm dev");
    }

    /// The WSL builder rewrites `npm run` to `pnpm run` when the working dir
    /// carries a `pnpm-lock.yaml` (financiApp regression path).
    #[test]
    fn test_build_command_wsl_replaces_pkg_manager() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let working_dir = dir.path().to_string_lossy().to_string();

        let runner = LocalRunner::new(Target::Wsl);
        let mut svc = wsl_service("npm run dev");
        svc.working_dir = Some(working_dir);
        let cmd = runner.build_command(&svc, ".");
        let shell_cmd = cmd_args(&cmd).pop().unwrap();
        assert!(
            shell_cmd.ends_with("&& pnpm run dev"),
            "npm should be rewritten to pnpm: {shell_cmd}"
        );
    }

    /// A venv living in an ancestor directory (monorepo layout) is resolved
    /// for a nested working dir.
    #[test]
    fn test_resolve_python_venv_in_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir
            .path()
            .join(".venv")
            .join(if cfg!(target_os = "windows") {
                "Scripts"
            } else {
                "bin"
            });
        std::fs::create_dir_all(&scripts).unwrap();
        let exe_name = if cfg!(target_os = "windows") {
            "python.exe"
        } else {
            "python3"
        };
        std::fs::write(scripts.join(exe_name), "").unwrap();

        // Nested working dir two levels below the venv root.
        let nested = dir.path().join("gui").join("backend");
        std::fs::create_dir_all(&nested).unwrap();
        let working_dir = nested.to_string_lossy().to_string();

        let result = resolve_python_venv("python app.py", &working_dir);
        assert!(result.contains(".venv"), "ancestor venv missed: {result}");
        assert!(result.contains("app.py"), "args lost: {result}");
    }

    /// A venv tool (`uvicorn`) with no matching exe in any venv falls back to
    /// the original command unchanged.
    #[test]
    fn test_resolve_python_venv_tool_without_exe_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        // A venv dir exists but does NOT contain the requested tool exe.
        let scripts = dir
            .path()
            .join(".venv")
            .join(if cfg!(target_os = "windows") {
                "Scripts"
            } else {
                "bin"
            });
        std::fs::create_dir_all(&scripts).unwrap();
        let working_dir = dir.path().to_string_lossy().to_string();

        assert_eq!(
            resolve_python_venv("uvicorn main:app", &working_dir),
            "uvicorn main:app"
        );
    }
}
