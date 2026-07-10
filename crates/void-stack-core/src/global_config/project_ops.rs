use std::path::{Path, PathBuf};

use super::GlobalConfig;
use crate::error::{Result, VoidStackError};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Find a project by name in the global config.
pub fn find_project<'a>(config: &'a GlobalConfig, name: &str) -> Option<&'a Project> {
    config
        .projects
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Remove a project by name. Returns true if found and removed.
pub fn remove_project(config: &mut GlobalConfig, name: &str) -> bool {
    let before = config.projects.len();
    config
        .projects
        .retain(|p| !p.name.eq_ignore_ascii_case(name));
    config.projects.len() < before
}

/// Rename and/or move a registered project WITHOUT losing derived data.
///
/// Where derived data actually lives (and what therefore needs migrating):
/// - **Vector index + contracts cache**: CENTRAL, keyed by project NAME
///   (`<data_local_dir>/void-stack/indexes/<name>/` — meta.db, hnsw/,
///   contracts.json). A rename moves this directory.
/// - **Structural graph**: INSIDE the project (`.void-stack/structural.db`),
///   so a directory move carries it automatically — except WSL UNC projects,
///   whose graph lives centrally under `void-stack/structural/<name>/` and
///   is moved on rename.
/// - **Trust approval**: user config dir, keyed by canonical PATH + command
///   digest. Re-keyed on move; the approval survives because the command set
///   the user blessed is unchanged (modulo rewritten working dirs).
/// - **Git post-commit hook**: contains the literal project name
///   (`void index <name>`), rewritten on rename.
/// - **Watch registry**: in-memory, keyed by path — callers holding watchers
///   re-key via `vector_index::unwatch_project`/`watch_project` (see the
///   MCP/desktop surfaces).
///
/// Nothing is re-indexed or rebuilt: the point is preserving work.
/// Returns the updated project plus a human-readable migration log.
pub fn update_project(
    old_name: &str,
    new_name: Option<&str>,
    new_path: Option<&str>,
) -> Result<(Project, Vec<String>)> {
    let mut config = super::load_global_config()?;
    let central = central_data_base();
    let result = update_project_in(&mut config, old_name, new_name, new_path, &central)?;
    super::save_global_config(&config)?;
    Ok(result)
}

/// Worker behind [`update_project`]: operates on an in-memory config and an
/// explicit central data dir (testable without touching the user's real
/// registry). The caller persists the config afterwards.
pub fn update_project_in(
    config: &mut GlobalConfig,
    old_name: &str,
    new_name: Option<&str>,
    new_path: Option<&str>,
    central_data_dir: &Path,
) -> Result<(Project, Vec<String>)> {
    let idx = config
        .projects
        .iter()
        .position(|p| p.name.eq_ignore_ascii_case(old_name))
        .ok_or_else(|| VoidStackError::ProjectNotFound(old_name.to_string()))?;
    let old_project = config.projects[idx].clone();

    // ── Validate ────────────────────────────────────────────
    let new_name = new_name.map(str::trim).filter(|n| !n.is_empty());
    if let Some(name) = new_name
        && !name.eq_ignore_ascii_case(&old_project.name)
        && config
            .projects
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(name))
    {
        return Err(VoidStackError::InvalidConfig(format!(
            "a project named '{}' already exists",
            name
        )));
    }
    if let Some(path) = new_path {
        let clean = strip_win_prefix(path);
        if !Path::new(&clean).is_dir() {
            return Err(VoidStackError::InvalidConfig(format!(
                "new path does not exist or is not a directory: {}",
                path
            )));
        }
    }

    let final_name = new_name.unwrap_or(&old_project.name).to_string();
    let final_path = new_path.unwrap_or(&old_project.path).to_string();
    let renamed = final_name != old_project.name;
    let moved = final_path != old_project.path;
    let mut log: Vec<String> = Vec::new();

    // ── Build the updated project ───────────────────────────
    let mut updated = old_project.clone();
    updated.name = final_name.clone();
    updated.path = final_path.clone();
    if moved {
        // Service working dirs that pointed inside the old tree follow it.
        for svc in &mut updated.services {
            if let Some(wd) = &svc.working_dir
                && let Some(rest) = wd.strip_prefix(&old_project.path)
            {
                svc.working_dir = Some(format!("{}{}", final_path, rest));
            }
        }
    }

    // ── Migrate derived data (registry saved LAST so a failed step can
    //    simply be retried) ───────────────────────────────────
    if renamed {
        for (sub, what) in [
            ("indexes", "semantic index"),
            ("structural", "WSL structural graph"),
        ] {
            let base = central_data_dir.join(sub);
            let from = base.join(&old_project.name);
            let to = base.join(&final_name);
            if from.is_dir() {
                if to.exists() {
                    return Err(VoidStackError::InvalidConfig(format!(
                        "cannot move {}: {} already exists",
                        what,
                        to.display()
                    )));
                }
                std::fs::rename(&from, &to).map_err(|e| {
                    VoidStackError::InvalidConfig(format!(
                        "failed to move {} {} -> {}: {}",
                        what,
                        from.display(),
                        to.display(),
                        e
                    ))
                })?;
                log.push(format!("{} moved to {}", what, to.display()));
            }
        }

        if let Some(line) = rewrite_git_hook(&updated, &old_project.name)? {
            log.push(line);
        }
    }

    if moved {
        let old_dir = PathBuf::from(strip_win_prefix(&old_project.path));
        let new_dir = PathBuf::from(strip_win_prefix(&final_path));
        if crate::config::rekey_trusted_project(&old_dir, &old_project, &new_dir, &updated)? {
            log.push("trust approval re-keyed to the new path".to_string());
        }
    }

    // ── Update the registry entry (caller persists) ─────────
    config.projects[idx] = updated.clone();

    Ok((updated, log))
}

/// Mirror of `vector_index::stats::index_dir`'s base, available without the
/// "vector" feature so registry maintenance never depends on it.
fn central_data_base() -> PathBuf {
    crate::global_config::data_base_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("void-stack")
}

/// Rewrite the auto-generated post-commit hook when the project is renamed:
/// it invokes `void index <name>` / `void graph-build <name>` by literal name.
fn rewrite_git_hook(project: &Project, old_name: &str) -> Result<Option<String>> {
    let hook_path = PathBuf::from(strip_win_prefix(&project.path))
        .join(".git")
        .join("hooks")
        .join("post-commit");
    let Ok(content) = std::fs::read_to_string(&hook_path) else {
        return Ok(None);
    };
    let mut rewritten = content.clone();
    for cmd in ["void index", "void graph-build"] {
        rewritten = rewritten.replace(
            &format!("{} {}", cmd, old_name),
            &format!("{} {}", cmd, project.name),
        );
    }
    if rewritten == content {
        return Ok(None);
    }
    std::fs::write(&hook_path, rewritten)
        .map_err(|e| VoidStackError::InvalidConfig(format!("failed to rewrite git hook: {}", e)))?;
    Ok(Some(
        "git post-commit hook updated to the new name".to_string(),
    ))
}

/// Remove a service from a project by name. Returns true if found and removed.
pub fn remove_service(config: &mut GlobalConfig, project_name: &str, service_name: &str) -> bool {
    if let Some(proj) = config
        .projects
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(project_name))
    {
        let before = proj.services.len();
        proj.services
            .retain(|s| !s.name.eq_ignore_ascii_case(service_name));
        proj.services.len() < before
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Service, Target};

    fn project(name: &str, path: &str) -> Project {
        Project {
            name: name.into(),
            description: String::new(),
            path: path.into(),
            project_type: None,
            tags: vec![],
            services: vec![Service {
                name: "api".into(),
                command: "cargo run".into(),
                target: Target::native(),
                working_dir: Some(format!("{}/srv", path)),
                enabled: true,
                env_vars: vec![],
                depends_on: vec![],
                docker: None,
            }],
            hooks: None,
        }
    }

    #[test]
    fn test_update_project_validations() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = project("alpha", &dir.path().join("alpha").to_string_lossy());
        let p2 = project("beta", &dir.path().join("beta").to_string_lossy());
        let mut config = GlobalConfig {
            projects: vec![p1, p2],
            ..Default::default()
        };
        let central = dir.path().join("central");

        // Unknown project
        assert!(update_project_in(&mut config, "nope", Some("x"), None, &central).is_err());
        // Name collision
        assert!(update_project_in(&mut config, "alpha", Some("BETA"), None, &central).is_err());
        // Path must exist
        assert!(
            update_project_in(&mut config, "alpha", None, Some("/no/such/dir"), &central).is_err()
        );
    }

    /// End-to-end acceptance: temp project with a structural graph, a
    /// central index dir, a trust approval and a post-commit hook → move
    /// the directory with fs::rename + update_project with new name AND
    /// path → everything is found at its new home WITHOUT re-indexing,
    /// and no stale old-name entries remain.
    #[cfg(feature = "structural")]
    #[test]
    fn test_update_project_end_to_end_rename_and_move() {
        let root = tempfile::tempdir().unwrap();
        let old_dir = root.path().join("old-home");
        let new_dir = root.path().join("new-home");
        let central = root.path().join("central");

        // Project source + git hook with the literal old name.
        std::fs::create_dir_all(old_dir.join(".git/hooks")).unwrap();
        std::fs::write(
            old_dir.join("lib.rs"),
            "pub fn rename_survivor() { helper_fn(); }\npub fn helper_fn() {}\n",
        )
        .unwrap();
        std::fs::write(
            old_dir.join(".git/hooks/post-commit"),
            "#!/bin/sh\n# Auto-generated by void-stack\n(void index proj-old --git-base HEAD~1 && void graph-build proj-old) 2>/dev/null &\n",
        )
        .unwrap();

        // Central index data keyed by NAME.
        std::fs::create_dir_all(central.join("indexes/proj-old")).unwrap();
        std::fs::write(central.join("indexes/proj-old/meta.db"), b"MARKER").unwrap();

        let old_path = old_dir.to_string_lossy().to_string();
        let old_project = project("proj-old", &old_path);

        // Structural graph (lives inside the project dir).
        crate::structural::build_structural_graph(&old_project, true).unwrap();
        assert!(old_dir.join(".void-stack/structural.db").exists());

        // Trust approval keyed by canonical path (isolated store).
        let store = root.path().join("trust.json");
        // SAFETY: test-only env override; no concurrent test reads it.
        unsafe { std::env::set_var("VOID_STACK_TRUST_STORE", &store) };
        crate::config::mark_project_trusted(&old_dir, &old_project).unwrap();
        assert!(crate::config::is_project_trusted(&old_dir, &old_project));

        // ── The move the user actually performs ──
        std::fs::rename(&old_dir, &new_dir).unwrap();

        let mut config = GlobalConfig {
            projects: vec![old_project.clone()],
            ..Default::default()
        };
        let new_path = new_dir.to_string_lossy().to_string();
        let (updated, log) = update_project_in(
            &mut config,
            "proj-old",
            Some("proj-new"),
            Some(&new_path),
            &central,
        )
        .unwrap();

        // Registry: renamed in place, no stale old entry.
        assert_eq!(config.projects.len(), 1);
        assert_eq!(config.projects[0].name, "proj-new");
        assert_eq!(config.projects[0].path, new_path);
        // Service working dir followed the move.
        assert_eq!(
            updated.services[0].working_dir.as_deref(),
            Some(format!("{}/srv", new_path).as_str())
        );

        // Central index moved with its content — no re-index.
        assert!(!central.join("indexes/proj-old").exists());
        assert_eq!(
            std::fs::read(central.join("indexes/proj-new/meta.db")).unwrap(),
            b"MARKER"
        );

        // Hook rewritten to the new name.
        let hook = std::fs::read_to_string(new_dir.join(".git/hooks/post-commit")).unwrap();
        assert!(hook.contains("void index proj-new"));
        assert!(hook.contains("void graph-build proj-new"));
        assert!(!hook.contains("proj-old"));

        // Trust approval survived the move under the new key.
        assert!(crate::config::is_project_trusted(&new_dir, &updated));
        assert!(!crate::config::is_project_trusted(&old_dir, &old_project));
        unsafe { std::env::remove_var("VOID_STACK_TRUST_STORE") };

        // Structural graph queryable at the new location WITHOUT rebuild.
        let conn = crate::structural::open_db(&updated).unwrap();
        let hits = crate::structural::query::search_nodes(&conn, "rename_survivor", 5);
        assert!(
            !hits.is_empty(),
            "structural graph must survive the move untouched"
        );

        assert!(log.iter().any(|l| l.contains("semantic index")));
        assert!(log.iter().any(|l| l.contains("hook")));
        assert!(log.iter().any(|l| l.contains("trust")));
    }
}
