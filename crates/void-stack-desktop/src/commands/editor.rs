//! Open a project file in the user's code editor at a given line.

use std::path::{Path, PathBuf};
use std::process::Command;

use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

/// Resolve an untrusted, project-relative `file` to an absolute path that is
/// proven to live inside `root`. Rejects absolute paths and `..` traversal,
/// canonicalizes both sides, and verifies containment — so a spoofed message
/// from the graph viewer can never open a file outside the project.
fn resolve_in_root(root: &str, file: &str) -> Result<PathBuf, String> {
    let rel = file.replace('\\', "/");
    if Path::new(&rel).is_absolute() || rel.split('/').any(|seg| seg == "..") {
        return Err("invalid file path".to_string());
    }
    let root_real =
        std::fs::canonicalize(root).map_err(|e| format!("project root unreadable: {}", e))?;
    let abs_real = std::fs::canonicalize(root_real.join(&rel))
        .map_err(|e| format!("file not found: {}", e))?;
    if !abs_real.starts_with(&root_real) {
        return Err("path escapes project root".to_string());
    }
    Ok(abs_real)
}

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

    // `file` arrives from the graph viewer (a sandboxed iframe) — untrusted.
    let abs = resolve_in_root(&strip_win_prefix(&proj.path), &file)?;

    let goto = format!("{}:{}", abs.display(), line.unwrap_or(1));
    // A canonical absolute path can't begin with '-', but be explicit so it
    // can never be parsed as an editor flag.
    if goto.starts_with('-') {
        return Err("invalid path".to_string());
    }

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

#[cfg(test)]
mod tests {
    use super::resolve_in_root;

    #[test]
    fn test_resolve_in_root_allows_inside_and_rejects_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        std::fs::create_dir_all(dir.path().join("lib/features")).unwrap();
        std::fs::write(dir.path().join("lib/features/a.dart"), "x").unwrap();

        // In-project file resolves.
        let ok = resolve_in_root(&root, "lib/features/a.dart").unwrap();
        assert!(ok.ends_with("a.dart"));

        // Parent traversal is rejected before touching the FS.
        assert!(resolve_in_root(&root, "../../../../etc/passwd").is_err());
        assert!(resolve_in_root(&root, "lib/../../escape.txt").is_err());
        // Absolute paths are rejected.
        assert!(resolve_in_root(&root, "/etc/passwd").is_err());
        // Non-existent in-project file fails (canonicalize) but isn't a traversal.
        assert!(resolve_in_root(&root, "lib/missing.dart").is_err());
    }
}
