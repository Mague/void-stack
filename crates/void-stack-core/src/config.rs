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

/// Relative path of the marker recording that the user approved executing
/// this project's configured service commands.
const TRUST_MARKER_DIR: &str = ".void-stack";
const TRUST_MARKER_FILE: &str = "trusted";

/// True when the user already confirmed that this project's service
/// commands may be executed (one-time confirmation).
pub fn is_project_trusted(project_dir: &Path) -> bool {
    project_dir
        .join(TRUST_MARKER_DIR)
        .join(TRUST_MARKER_FILE)
        .exists()
}

/// Record the user's approval to execute this project's service commands.
pub fn mark_project_trusted(project_dir: &Path) -> Result<()> {
    let dir = project_dir.join(TRUST_MARKER_DIR);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join(TRUST_MARKER_FILE),
        "Service commands from void-stack.toml were approved for execution on this machine.\n",
    )?;
    Ok(())
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
    } else if path.join("docker-compose.yml").exists()
        || path.join("docker-compose.yaml").exists()
        || path.join("Dockerfile").exists()
    {
        ProjectType::Docker
    } else {
        ProjectType::Unknown
    }
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
