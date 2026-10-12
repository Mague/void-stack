//! Cross-platform process utilities.
//!
//! On Windows, child processes inherit the console by default, causing visible
//! cmd.exe windows to flash when spawning tools like `git`, `python`, `clippy`,
//! etc. This module provides helpers to suppress those windows.
//!
//! Also provides [`shell_command`] for running shell strings cross-platform
//! (`cmd /c` on Windows, `sh -c` on Unix).

/// Windows: CREATE_NO_WINDOW flag prevents child processes from opening
/// visible console windows.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Extension trait to hide console windows on Windows.
///
/// Works with both `std::process::Command` and `tokio::process::Command`.
///
/// # Example
/// ```ignore
/// use void_stack_core::process_util::HideWindow;
///
/// let output = std::process::Command::new("git")
///     .args(["status"])
///     .hide_window()
///     .output();
/// ```
pub trait HideWindow {
    fn hide_window(&mut self) -> &mut Self;
}

impl HideWindow for std::process::Command {
    #[cfg(target_os = "windows")]
    fn hide_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(target_os = "windows"))]
    fn hide_window(&mut self) -> &mut Self {
        self
    }
}

impl HideWindow for tokio::process::Command {
    #[cfg(target_os = "windows")]
    fn hide_window(&mut self) -> &mut Self {
        // tokio::process::Command exposes creation_flags directly
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(target_os = "windows"))]
    fn hide_window(&mut self) -> &mut Self {
        self
    }
}

/// Create a [`tokio::process::Command`] that executes a shell string
/// cross-platform: `cmd /c <shell_str>` on Windows, `sh -c <shell_str>` on Unix.
///
/// # Trust model
/// `shell_str` is executed verbatim with the caller's privileges. By design
/// void-stack is a launcher for commands the user configured themselves
/// (`void-stack.toml`, registered services) — treat every string that
/// reaches this function as trusted input and never feed it data from an
/// unreviewed repository or remote source.
pub fn shell_command(shell_str: &str) -> tokio::process::Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.args(["/c", shell_str]);
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", shell_str]);
        cmd
    }
}

/// Create a [`std::process::Command`] that executes a shell string
/// cross-platform: `cmd /c <shell_str>` on Windows, `sh -c <shell_str>` on Unix.
///
/// # Trust model
/// Same contract as [`shell_command`]: the string runs verbatim with the
/// caller's privileges and must come from user-reviewed configuration.
pub fn shell_command_sync(shell_str: &str) -> std::process::Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/c", shell_str]);
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", shell_str]);
        cmd
    }
}

/// Check if a process with the given PID is alive, cross-platform.
/// Windows asks `tasklist`; Unix (Linux and macOS) uses `kill(pid, 0)`.
pub fn is_pid_alive_sync(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
            .hide_window()
            .output();
        match output {
            Ok(out) => tasklist_lists_pid(&String::from_utf8_lossy(&out.stdout), pid),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        unix_pid_alive(pid)
    }
}

/// Async version of [`is_pid_alive_sync`].
pub async fn is_pid_alive_async(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("tasklist");
        cmd.args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"]);
        cmd.hide_window();
        match cmd.output().await {
            Ok(output) => tasklist_lists_pid(&String::from_utf8_lossy(&output.stdout), pid),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        unix_pid_alive(pid)
    }
}

/// `kill(pid, 0)` probes a process without signalling it — but only for a
/// real, positive pid: `pid as i32` turned anything above `i32::MAX` into a
/// negative number (-1 = "every process I may signal") and 0 means "my
/// process group", both of which report alive.
#[cfg(not(target_os = "windows"))]
fn unix_pid_alive(pid: u32) -> bool {
    match i32::try_from(pid) {
        Ok(p) if p > 0 => unsafe { libc::kill(p, 0) == 0 },
        _ => false,
    }
}

/// Whether `tasklist /FO CSV /NH` output has a row for exactly `pid`.
/// Matching the pid as a substring of the whole output (the old check)
/// made pid 12 "alive" whenever a row mentioned 1234 or 12,000 K.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn tasklist_lists_pid(stdout: &str, pid: u32) -> bool {
    let want = pid.to_string();
    stdout.lines().any(|line| {
        line.split("\",\"")
            .nth(1)
            .is_some_and(|field| field.trim_matches('"') == want)
    })
}

/// Platform-appropriate install hint for a given tool.
pub fn install_hint(tool: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        match tool {
            "python" => "winget install Python.Python.3".into(),
            "node" => "winget install OpenJS.NodeJS.LTS".into(),
            "docker" => "winget install Docker.DockerDesktop".into(),
            "go" => "winget install GoLang.Go".into(),
            "ollama" => "winget install Ollama.Ollama".into(),
            "rust" => "winget install Rustlang.Rust.MSVC".into(),
            "flutter" => "winget install Google.Flutter".into(),
            _ => format!("winget install {}", tool),
        }
    }
    #[cfg(target_os = "macos")]
    {
        match tool {
            "python" => "brew install python3".into(),
            "node" => "brew install node".into(),
            "docker" => "brew install --cask docker".into(),
            "go" => "brew install go".into(),
            "ollama" => "brew install ollama".into(),
            "rust" => "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh".into(),
            "flutter" => "brew install --cask flutter".into(),
            _ => format!("brew install {}", tool),
        }
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        match tool {
            "python" => {
                "sudo apt install python3 (Debian/Ubuntu) or sudo dnf install python3 (Fedora)"
                    .into()
            }
            "node" => {
                "sudo apt install nodejs (Debian/Ubuntu) or sudo dnf install nodejs (Fedora)".into()
            }
            "docker" => "https://docs.docker.com/engine/install/".into(),
            "go" => {
                "sudo apt install golang-go (Debian/Ubuntu) or sudo dnf install golang (Fedora)"
                    .into()
            }
            "ollama" => "curl -fsSL https://ollama.com/install.sh | sh".into(),
            "rust" => "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh".into(),
            "flutter" => "https://docs.flutter.dev/get-started/install/linux".into(),
            _ => format!("Install {} using your package manager", tool),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tasklist_lists_pid_matches_exact_pid_only() {
        let out = "\"void-stack-mcp.exe\",\"1234\",\"Console\",\"1\",\"12,000 K\"\r\n";
        assert!(tasklist_lists_pid(out, 1234));
        assert!(!tasklist_lists_pid(out, 12), "no substring matches");
        assert!(!tasklist_lists_pid(out, 1), "session column is not the pid");
        let none = "INFO: No tasks are running which match the specified criteria.\r\n";
        assert!(!tasklist_lists_pid(none, 1234));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_unix_pid_alive_rejects_non_positive_and_overflowing_pids() {
        assert!(unix_pid_alive(std::process::id()));
        assert!(!unix_pid_alive(0), "0 is the process group, not a pid");
        assert!(!unix_pid_alive(u32::MAX), "would become kill(-1, 0)");
    }

    #[test]
    fn test_install_hint_python() {
        let hint = install_hint("python");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_install_hint_node() {
        let hint = install_hint("node");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_install_hint_docker() {
        let hint = install_hint("docker");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_install_hint_go() {
        let hint = install_hint("go");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_install_hint_rust() {
        let hint = install_hint("rust");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_install_hint_flutter() {
        let hint = install_hint("flutter");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_install_hint_ollama() {
        let hint = install_hint("ollama");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_install_hint_unknown() {
        let hint = install_hint("nonexistent-tool-xyz");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_shell_command_sync_returns_command() {
        let cmd = shell_command_sync("echo hello");
        // Just verify it doesn't panic — the command object is valid
        let _ = cmd;
    }
}
