//! Re-index triggers: the per-project file watcher and the git hook.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use super::index_project_background_then_structural;
use crate::error::IndexError;
use crate::ignore::VoidIgnore;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Default quiet window before a watcher-triggered re-index. Reacting
/// 500 ms after every save (each run hashes the whole project and, before
/// 0.33, rebuilt an HNSW graph) kept the CPU busy all day while editing; a
/// few seconds coalesces an editing burst into one run. Override per
/// project: `[index] watch_debounce_ms`.
const WATCH_DEBOUNCE_MS: u64 = 5_000;

/// Global watch registry: project_path → active watcher.
static WATCH_REGISTRY: OnceLock<Mutex<HashMap<String, RecommendedWatcher>>> = OnceLock::new();

fn watch_registry() -> &'static Mutex<HashMap<String, RecommendedWatcher>> {
    WATCH_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start watching a project directory. File changes trigger an incremental
/// re-index ~500 ms after the last event (debounced to absorb bursts from
/// autoformat or save-all). Call `unwatch_project` to stop.
pub fn watch_project(project: &Project) -> Result<(), IndexError> {
    let project_path = PathBuf::from(strip_win_prefix(&project.path));
    let project_clone = project.clone();
    let key = project.path.clone();

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx)
        .map_err(|e| IndexError::Other(format!("failed to create watcher: {}", e)))?;

    watcher
        .watch(&project_path, RecursiveMode::Recursive)
        .map_err(|e| IndexError::Other(format!("failed to watch path: {}", e)))?;

    let watch_path = project_path.clone();
    let ignore = VoidIgnore::load(&watch_path);
    let debounce = std::time::Duration::from_millis(
        crate::project_config::ProjectConfig::load(&watch_path)
            .index
            .watch_debounce_ms
            .unwrap_or(WATCH_DEBOUNCE_MS),
    );

    std::thread::spawn(move || {
        let mut last_event = std::time::Instant::now();
        let mut pending = false;

        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Ok(event)) => {
                    let is_relevant = matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                    ) && event.paths.iter().any(|p| {
                        let rel = p.strip_prefix(&watch_path).unwrap_or(p);
                        let rel_str = rel.to_string_lossy();
                        !ignore.is_ignored(&rel_str)
                            && !rel_str.contains(".void-stack")
                            && !rel_str.contains("target/")
                            && !rel_str.contains("node_modules/")
                            && !rel_str.contains(".git/")
                    });

                    if is_relevant {
                        last_event = std::time::Instant::now();
                        pending = true;
                    }
                }
                Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }

            if pending && last_event.elapsed() >= debounce {
                pending = false;
                // Filesystem watch reacts to save events, not commits. We
                // want the indexer to use SHA-256 to figure out which files
                // actually changed (which is what passing `None` does);
                // `Some("HEAD")` would diff HEAD against the working tree
                // and silently re-index nothing whenever everything is
                // already committed.
                index_project_background_then_structural(&project_clone, false, None);

                // Optional board step: mirror TODO/FIXME/HACK markers into
                // BOARD.md when `[board] todo_sync_on_watch = true` in the
                // project's .void-config.
                let cfg = crate::project_config::ProjectConfig::load(&watch_path);
                if cfg.board.todo_sync_on_watch
                    && let Err(e) = crate::todosync::sync_todos(&project_clone)
                {
                    tracing::warn!("todo-sync on watch failed: {}", e);
                }
            }
        }
    });

    let mut registry = watch_registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.insert(key, watcher);

    Ok(())
}

/// Stop watching a project (idempotent).
pub fn unwatch_project(project: &Project) {
    let mut registry = watch_registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.remove(&project.path);
}

/// Whether `watch_project` is currently active for this project.
pub fn is_watching(project: &Project) -> bool {
    let registry = watch_registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.contains_key(&project.path)
}

/// Install a git `post-commit` hook that triggers an incremental re-index
/// after each commit. Appends to an existing hook if present and is a no-op
/// when the hook already contains a `void index` line. Returns an error if
/// the directory is not a git repo.
pub fn install_git_hook(project: &Project) -> Result<(), IndexError> {
    let project_path = PathBuf::from(strip_win_prefix(&project.path));
    let hooks_dir = project_path.join(".git").join("hooks");

    if !hooks_dir.exists() {
        return Err(IndexError::Other("not a git repository".to_string()));
    }

    let hook_path = hooks_dir.join("post-commit");
    // HEAD~1 compares the freshly-made commit against its parent, which is
    // what we want post-commit. Using plain HEAD would diff HEAD against
    // the working tree — empty right after a commit, so nothing re-indexes.
    // The structural graph is rebuilt right after (incremental by SHA-256)
    // so BOTH indexes stay fresh — a stale structural graph silently
    // produces empty GraphRAG context.
    let command_line = format!(
        "(void index {p} --git-base HEAD~1 && void graph-build {p}) 2>/dev/null &",
        p = project.name
    );
    let hook_line = format!(
        "#!/bin/sh\n# Auto-generated by void-stack\n{}\n",
        command_line
    );

    let existing = std::fs::read_to_string(&hook_path).unwrap_or_default();
    if existing.contains("void graph-build") {
        return Ok(());
    }

    let new_content = if existing.is_empty() {
        hook_line
    } else if existing.contains("void index") {
        // Upgrade a hook written by an older version (index only): replace
        // its generated line with the combined index+graph-build command.
        existing
            .lines()
            .map(|l| {
                if l.contains("void index") && !l.contains("graph-build") {
                    command_line.clone()
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    } else {
        format!("{}\n{}", existing.trim_end(), hook_line)
    };

    std::fs::write(&hook_path, new_content)
        .map_err(|e| IndexError::Other(format!("failed to write hook: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)
            .map_err(IndexError::from)?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms).map_err(IndexError::from)?;
    }

    Ok(())
}
