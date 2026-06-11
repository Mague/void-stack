//! Open a project file in the user's code editor at a given line.

use std::process::Command;

use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

/// Editor CLIs that accept `-g <file>:<line>` (VS Code and its forks). Bare
/// names cover a normal PATH; the absolute paths cover GUI launches on macOS
/// where the app doesn't inherit the shell PATH and `code` isn't found.
const EDITOR_GOTO: &[&str] = &[
    "code",
    "cursor",
    "windsurf",
    "code-insiders",
    "/usr/local/bin/code",
    "/opt/homebrew/bin/code",
    "/usr/local/bin/cursor",
    "/opt/homebrew/bin/cursor",
    "/usr/local/bin/windsurf",
    "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
    "/Applications/Cursor.app/Contents/Resources/app/bin/cursor",
    "/Applications/Windsurf.app/Contents/Resources/app/bin/windsurf",
];

/// Open `file` (project-relative) at `line` in a code editor. Tries the
/// VS Code family with `-g file:line`, then falls back to the OS default
/// opener (no line positioning).
#[tauri::command]
pub fn open_in_editor_cmd(project: String, file: String, line: Option<u32>) -> Result<(), String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let root = strip_win_prefix(&proj.path);
    let abs = std::path::Path::new(&root).join(file.replace('\\', "/"));
    if !abs.exists() {
        return Err(format!("file not found: {}", abs.display()));
    }
    let goto = format!("{}:{}", abs.display(), line.unwrap_or(1));

    for ed in EDITOR_GOTO {
        if Command::new(ed).arg("-g").arg(&goto).spawn().is_ok() {
            return Ok(());
        }
    }

    // No editor CLI found — open with the OS default (no line jump).
    #[cfg(target_os = "macos")]
    let fallback = Command::new("open").arg(&abs).spawn();
    #[cfg(target_os = "windows")]
    let fallback = Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(&abs)
        .spawn();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let fallback = Command::new("xdg-open").arg(&abs).spawn();

    fallback
        .map(|_| ())
        .map_err(|e| format!("no editor available to open {}: {}", abs.display(), e))
}
