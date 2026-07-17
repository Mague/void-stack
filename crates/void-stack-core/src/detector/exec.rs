//! Helpers for running external commands during dependency checks:
//! user PATH resolution (macOS GUI apps) and timeout-bounded execution.

use std::sync::OnceLock;
use std::time::Duration;

/// Default timeout for running external commands.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

static USER_SHELL_PATH: OnceLock<String> = OnceLock::new();

/// Resolve the full user PATH from a login shell on macOS.
///
/// GUI apps launched from Finder/Dock inherit a minimal PATH
/// (`/usr/bin:/bin:/usr/sbin:/sbin`) that excludes Homebrew, NVM, Volta,
/// Cargo and other developer tool directories.
///
/// Two-layer approach:
/// 1. Spawn a **login + interactive** shell (`-li`) with `TERM` set so that
///    tools like NVM, Volta and pyenv actually initialise (they check `$-`
///    for `i` or `$TERM` before modifying PATH). The result is validated by
///    counting colon separators (>5 indicates a real developer PATH).
/// 2. **Filesystem fallback**: directly probe well-known directories for
///    Homebrew, Cargo, Volta, pyenv, rbenv and NVM. This works even when no
///    shell initialisation runs at all.
///
/// Result is cached for the process lifetime via `OnceLock`.
fn get_user_shell_path() -> &'static str {
    USER_SHELL_PATH.get_or_init(|| {
        // Layer 1: login + interactive shell with TERM forced
        for shell in &["/bin/zsh", "/bin/bash"] {
            if let Ok(output) = std::process::Command::new(shell)
                .args(["-li", "-c", "echo $PATH"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .env("TERM", "xterm-256color")
                .output()
            {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // A real developer PATH has many segments; the Finder-inherited
                // one has ~4 (`/usr/bin:/bin:/usr/sbin:/sbin`).
                if path.matches(':').count() > 5 {
                    #[cfg(debug_assertions)]
                    eprintln!("[void-stack] PATH resolved via shell ({shell}): {path}");
                    return path;
                }
            }
        }

        // Layer 2: build PATH by probing the filesystem directly
        let home = std::env::var("HOME").unwrap_or_default();

        let mut paths: Vec<String> = vec![
            // Homebrew — Apple Silicon
            "/opt/homebrew/bin".into(),
            "/opt/homebrew/sbin".into(),
            // Homebrew — Intel
            "/usr/local/bin".into(),
            "/usr/local/sbin".into(),
            // Rust / Cargo
            format!("{home}/.cargo/bin"),
            // Volta
            format!("{home}/.volta/bin"),
            // pyenv
            format!("{home}/.pyenv/bin"),
            format!("{home}/.pyenv/shims"),
            // rbenv
            format!("{home}/.rbenv/bin"),
            format!("{home}/.rbenv/shims"),
        ];

        // NVM — find the latest installed node version
        let nvm_dir = std::env::var("NVM_DIR").unwrap_or_else(|_| format!("{home}/.nvm"));
        let nvm_versions = format!("{nvm_dir}/versions/node");
        if let Ok(entries) = std::fs::read_dir(&nvm_versions) {
            let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            versions.sort_by_key(|e| e.file_name());
            if let Some(latest) = versions.last() {
                paths.push(format!("{}/bin", latest.path().display()));
            }
        }

        // Keep only paths that actually exist on this machine
        let valid: Vec<String> = paths
            .into_iter()
            .filter(|p| std::path::Path::new(p).exists())
            .collect();

        let current = std::env::var("PATH").unwrap_or_default();
        let resolved = if valid.is_empty() {
            current
        } else {
            format!("{}:{}", valid.join(":"), current)
        };

        #[cfg(debug_assertions)]
        eprintln!("[void-stack] PATH resolved via filesystem fallback: {resolved}");

        resolved
    })
}

/// Run a command with a timeout and return its stdout as a string.
/// Returns None if the command fails or times out.
pub(crate) async fn run_cmd(program: &str, args: &[&str]) -> Option<String> {
    use crate::process_util::HideWindow;
    let result = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .env("PATH", get_user_shell_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .hide_window()
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        _ => None,
    }
}

/// Run a command and return stdout even if exit code is non-zero.
pub(crate) async fn run_cmd_any(program: &str, args: &[&str]) -> Option<String> {
    use crate::process_util::HideWindow;
    let result = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .env("PATH", get_user_shell_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .hide_window()
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => Some(String::from_utf8_lossy(&output.stdout).trim().to_string()),
        _ => None,
    }
}
