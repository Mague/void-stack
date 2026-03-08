use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{DevLaunchError, Result};
use crate::model::Project;

const GLOBAL_CONFIG_FILENAME: &str = "config.toml";
const APP_DIR_NAME: &str = "devlaunch";

/// Wrapper for the global config containing multiple projects.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct GlobalConfig {
    #[serde(default)]
    pub projects: Vec<Project>,
}

/// Get the global config directory (%LOCALAPPDATA%\devlaunch\ on Windows).
pub fn global_config_dir() -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| DevLaunchError::ConfigNotFound("Cannot determine local data directory".into()))?;
    Ok(base.join(APP_DIR_NAME))
}

/// Full path to the global config file.
pub fn global_config_path() -> Result<PathBuf> {
    Ok(global_config_dir()?.join(GLOBAL_CONFIG_FILENAME))
}

/// Load the global config. Returns empty config if file doesn't exist.
pub fn load_global_config() -> Result<GlobalConfig> {
    let path = global_config_path()?;
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let content = fs::read_to_string(&path)?;
    let config: GlobalConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Save the global config, creating the directory if needed.
pub fn save_global_config(config: &GlobalConfig) -> Result<()> {
    let dir = global_config_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    let path = dir.join(GLOBAL_CONFIG_FILENAME);
    let content = toml::to_string_pretty(config)
        .map_err(|e| DevLaunchError::InvalidConfig(e.to_string()))?;
    fs::write(&path, content)?;
    Ok(())
}

/// Find a project by name in the global config.
pub fn find_project<'a>(config: &'a GlobalConfig, name: &str) -> Option<&'a Project> {
    config.projects.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Remove a project by name. Returns true if found and removed.
pub fn remove_project(config: &mut GlobalConfig, name: &str) -> bool {
    let before = config.projects.len();
    config.projects.retain(|p| !p.name.eq_ignore_ascii_case(name));
    config.projects.len() < before
}

/// Scan a directory for sub-projects (monorepo detection).
/// Returns a list of (subdir_name, path, detected_type).
pub fn scan_subprojects(root: &Path) -> Vec<(String, PathBuf, crate::model::ProjectType)> {
    use crate::config::detect_project_type;

    let mut results = Vec::new();

    // Check root itself
    let root_type = detect_project_type(root);
    if root_type != crate::model::ProjectType::Unknown {
        let name = root.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".into());
        results.push((name, root.to_path_buf(), root_type));
    }

    // Scan immediate subdirectories
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden dirs and common non-project dirs
                if dir_name.starts_with('.')
                    || dir_name == "node_modules"
                    || dir_name == "target"
                    || dir_name == "__pycache__"
                    || dir_name == "venv"
                    || dir_name == ".venv"
                    || dir_name == "dist"
                    || dir_name == "build"
                {
                    continue;
                }

                let sub_type = detect_project_type(&path);
                if sub_type != crate::model::ProjectType::Unknown {
                    results.push((dir_name.clone(), path.clone(), sub_type));
                }

                // Also check one level deeper (e.g., backends/qwen3tts/)
                if let Ok(sub_entries) = fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_dir() {
                            let sub_name = sub_entry.file_name().to_string_lossy().to_string();
                            if sub_name.starts_with('.')
                                || sub_name == "node_modules"
                                || sub_name == "venv"
                                || sub_name == ".venv"
                                || sub_name == "__pycache__"
                            {
                                continue;
                            }
                            let deep_type = detect_project_type(&sub_path);
                            if deep_type != crate::model::ProjectType::Unknown {
                                results.push((
                                    format!("{}/{}", dir_name, sub_name),
                                    sub_path,
                                    deep_type,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    results
}

/// Suggest a default command based on project type.
pub fn default_command_for(pt: crate::model::ProjectType) -> String {
    use crate::model::ProjectType;
    match pt {
        ProjectType::Python => "python main.py".into(),
        ProjectType::Node => "npm run dev".into(),
        ProjectType::Rust => "cargo run".into(),
        ProjectType::Go => "go run .".into(),
        ProjectType::Docker => "docker compose up".into(),
        ProjectType::Unknown => "echo 'hello'".into(),
    }
}

/// Scan a WSL directory for sub-projects using a single WSL command.
/// Returns a list of (subdir_name, wsl_path, detected_type).
pub fn scan_wsl_subprojects(wsl_path: &str) -> Vec<(String, String, crate::model::ProjectType)> {
    use crate::model::ProjectType;

    // Single WSL call: find all project marker files up to depth 3
    let script = format!(
        r#"find '{}' -maxdepth 3 \( \
            -name 'Cargo.toml' -o \
            -name 'package.json' -o \
            -name 'requirements.txt' -o \
            -name 'pyproject.toml' -o \
            -name 'setup.py' -o \
            -name 'go.mod' -o \
            -name 'docker-compose.yml' -o \
            -name 'Dockerfile' \
        \) -not -path '*/node_modules/*' \
           -not -path '*/.venv/*' \
           -not -path '*/venv/*' \
           -not -path '*/target/*' \
           2>/dev/null"#,
        wsl_path
    );

    let output = std::process::Command::new("wsl.exe")
        .args(["-e", "bash", "-c", &script])
        .output();

    let markers = match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
        Err(_) => return Vec::new(),
    };

    // Group marker files by their parent directory
    let mut dir_markers: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for line in markers.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(parent) = line.rsplit_once('/').map(|(p, _)| p.to_string()) {
            let filename = line.rsplit('/').next().unwrap_or("").to_string();
            dir_markers.entry(parent).or_default().push(filename);
        }
    }

    // Determine project type for each directory
    let mut results = Vec::new();
    for (dir, files) in &dir_markers {
        let pt = if files.iter().any(|f| f == "Cargo.toml") {
            ProjectType::Rust
        } else if files.iter().any(|f| {
            f == "requirements.txt" || f == "pyproject.toml" || f == "setup.py"
        }) {
            ProjectType::Python
        } else if files.iter().any(|f| f == "package.json") {
            ProjectType::Node
        } else if files.iter().any(|f| f == "go.mod") {
            ProjectType::Go
        } else if files.iter().any(|f| {
            f == "docker-compose.yml" || f == "Dockerfile"
        }) {
            ProjectType::Docker
        } else {
            continue;
        };

        let rel = dir.strip_prefix(wsl_path).unwrap_or(dir);
        let rel = rel.trim_start_matches('/');
        let name = if rel.is_empty() {
            dir.rsplit('/').next().unwrap_or("root").to_string()
        } else {
            rel.to_string()
        };

        results.push((name, dir.clone(), pt));
    }

    // Sort by path for consistent output
    results.sort_by(|a, b| a.1.cmp(&b.1));
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_global_config_roundtrip() {
        use crate::model::*;

        let config = GlobalConfig {
            projects: vec![Project {
                name: "test-project".into(),
                description: "A test".into(),
                path: "F:\\test".into(),
                project_type: Some(ProjectType::Node),
                tags: vec![],
                services: vec![Service {
                    name: "web".into(),
                    command: "npm run dev".into(),
                    target: Target::Windows,
                    working_dir: Some("F:\\test\\frontend".into()),
                    enabled: true,
                    env_vars: vec![],
                    depends_on: vec![],
                }],
                hooks: None,
            }],
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: GlobalConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "test-project");
        assert_eq!(loaded.projects[0].services[0].working_dir.as_deref(), Some("F:\\test\\frontend"));
    }

    #[test]
    fn test_scan_subprojects() {
        let dir = tempdir().unwrap();
        // Create a Node root
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        // Create a Python subdir
        let backend = dir.path().join("backend");
        std::fs::create_dir(&backend).unwrap();
        std::fs::write(backend.join("requirements.txt"), "flask").unwrap();

        let results = scan_subprojects(dir.path());
        assert!(results.len() >= 2);
        // Should find Node at root and Python in backend/
        let types: Vec<_> = results.iter().map(|(_, _, t)| *t).collect();
        assert!(types.contains(&crate::model::ProjectType::Node));
        assert!(types.contains(&crate::model::ProjectType::Python));
    }
}
