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

/// Percent-encode a path for use in an editor `file://`-style URL (keeps the
/// unreserved set + `/`; everything else, including spaces, is escaped).
#[cfg(target_os = "macos")]
fn encode_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Editor CLIs that accept `-g <file>:<line>` (VS Code and its forks). Bare
/// names cover a normal PATH; the absolute paths cover GUI launches where the
/// app doesn't inherit the shell PATH and `code` isn't found.
#[cfg(not(target_os = "macos"))]
const EDITOR_GOTO: &[&str] = &[
    "code",
    "cursor",
    "windsurf",
    "code-insiders",
    "/usr/local/bin/code",
    "/usr/local/bin/cursor",
];

/// Open `file` (project-relative) at `line` in the user's code editor.
#[tauri::command]
pub fn open_in_editor_cmd(project: String, file: String, line: Option<u32>) -> Result<(), String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    // `file` arrives from the graph viewer (a sandboxed iframe) — untrusted.
    let abs = resolve_in_root(&strip_win_prefix(&proj.path), &file)?;
    open_at(&abs, line.unwrap_or(1))
}

/// macOS: launch via LaunchServices with an editor URL scheme. Unlike
/// spawning the `code` CLI (which inherits the app's session and flashed open
/// then closed), `open <scheme>://…` hands off to the OS so the editor stays
/// open, and the URL jumps to the line.
#[cfg(target_os = "macos")]
fn open_at(abs: &Path, line: u32) -> Result<(), String> {
    let enc = encode_path(&abs.to_string_lossy());
    for scheme in ["vscode", "cursor", "windsurf"] {
        let url = format!("{}://file/{}:{}", scheme, enc, line);
        if Command::new("open")
            .arg(&url)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Ok(());
        }
    }
    // No editor URL handler — open with the default app (no line jump).
    Command::new("open")
        .arg(abs)
        .status()
        .map(|_| ())
        .map_err(|e| format!("open failed: {}", e))
}

#[cfg(not(target_os = "macos"))]
fn open_at(abs: &Path, line: u32) -> Result<(), String> {
    use std::process::Stdio;

    let goto = format!("{}:{}", abs.display(), line);
    if goto.starts_with('-') {
        return Err("invalid path".to_string());
    }
    let spawn_detached = |mut c: Command| -> std::io::Result<()> {
        c.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c.spawn().map(|_| ())
    };
    for ed in EDITOR_GOTO {
        let mut c = Command::new(ed);
        c.arg("-g").arg(&goto);
        if spawn_detached(c).is_ok() {
            return Ok(());
        }
    }
    #[cfg(target_os = "windows")]
    let fb = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]).arg(abs);
        c
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let fb = {
        let mut c = Command::new("xdg-open");
        c.arg(abs);
        c
    };
    spawn_detached(fb).map_err(|e| format!("no editor available: {}", e))
}

#[cfg(test)]
mod tests {
    use super::{open_in_editor_cmd, resolve_in_root};
    use crate::commands::test_support;

    #[test]
    fn test_open_in_editor_unknown_project_errors() {
        let _g = test_support::config_guard();
        assert!(open_in_editor_cmd("Ghost".to_string(), "a.rs".to_string(), None).is_err());
    }

    #[test]
    fn test_open_in_editor_rejects_traversal_before_spawn() {
        let _g = test_support::config_guard();
        let dir = tempfile::tempdir().unwrap();
        test_support::register(test_support::project("Ed", dir.path()));

        // Traversal is rejected by resolve_in_root before any editor is spawned.
        assert!(
            open_in_editor_cmd("Ed".to_string(), "../../etc/passwd".to_string(), None).is_err()
        );
        assert!(open_in_editor_cmd("Ed".to_string(), "/etc/passwd".to_string(), None).is_err());
    }

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
