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
/// Works on Windows (tasklist), Linux (/proc), and macOS (kill -0).
pub fn is_pid_alive_sync(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .hide_window()
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout.contains(&pid.to_string())
            }
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // kill -0 checks if process exists without sending a signal.
        // Works on both Linux and macOS (unlike /proc which is Linux-only).
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}

/// Async version of [`is_pid_alive_sync`].
pub async fn is_pid_alive_async(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("tasklist");
        cmd.args(["/FI", &format!("PID eq {}", pid), "/NH"]);
        cmd.hide_window();
        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains(&pid.to_string())
            }
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
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
            "python" => "sudo apt install python3 (Debian/Ubuntu) or sudo dnf install python3 (Fedora)".into(),
            "node" => "sudo apt install nodejs (Debian/Ubuntu) or sudo dnf install nodejs (Fedora)".into(),
            "docker" => "https://docs.docker.com/engine/install/".into(),
            "go" => "sudo apt install golang-go (Debian/Ubuntu) or sudo dnf install golang (Fedora)".into(),
            "ollama" => "curl -fsSL https://ollama.com/install.sh | sh".into(),
            "rust" => "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh".into(),
            "flutter" => "https://docs.flutter.dev/get-started/install/linux".into(),
            _ => format!("Install {} using your package manager", tool),
        }
    }
}
