use std::path::{Path, PathBuf};

use crate::error::{VoidStackError, Result};
use crate::model::Project;

const CONFIG_FILENAME: &str = "void-stack.toml";

/// Load a project config from a TOML file.
pub fn load_project(path: &Path) -> Result<Project> {
    let config_path = resolve_config_path(path);
    let content = std::fs::read_to_string(&config_path).map_err(|_| {
        VoidStackError::ConfigNotFound(config_path.display().to_string())
    })?;
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

/// Auto-detect project type by inspecting files in the directory.
pub fn detect_project_type(path: &Path) -> crate::model::ProjectType {
    use crate::model::ProjectType;

    if path.join("Cargo.toml").exists() {
        ProjectType::Rust
    } else if path.join("pyproject.toml").exists()
        || path.join("requirements.txt").exists()
        || path.join("setup.py").exists()
    {
        ProjectType::Python
    } else if path.join("package.json").exists() {
        ProjectType::Node
    } else if path.join("go.mod").exists() {
        ProjectType::Go
    } else if path.join("pubspec.yaml").exists() {
        ProjectType::Flutter
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
        assert_eq!(detect_project_type(dir.path()), crate::model::ProjectType::Python);
    }

    #[test]
    fn test_detect_node() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_project_type(dir.path()), crate::model::ProjectType::Node);
    }

    #[test]
    fn test_detect_unknown() {
        let dir = tempdir().unwrap();
        assert_eq!(detect_project_type(dir.path()), crate::model::ProjectType::Unknown);
    }
}
