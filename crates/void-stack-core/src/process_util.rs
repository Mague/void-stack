//! Cross-platform process utilities.
//!
//! On Windows, child processes inherit the console by default, causing visible
//! cmd.exe windows to flash when spawning tools like `git`, `python`, `clippy`,
//! etc. This module provides helpers to suppress those windows.

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
