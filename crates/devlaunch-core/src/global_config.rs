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

    // Check root itself — only add if it has a runnable entrypoint
    let root_type = detect_project_type(root);
    if root_type != crate::model::ProjectType::Unknown && has_entrypoint(root_type, root) {
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

/// Check if a directory has a runnable entrypoint for its project type.
/// Prevents adding library/config-only directories as services.
fn has_entrypoint(pt: crate::model::ProjectType, dir: &Path) -> bool {
    use crate::model::ProjectType;
    match pt {
        ProjectType::Python => {
            // Must have at least one executable Python file
            ["main.py", "app.py", "server.py", "run.py", "manage.py"]
                .iter()
                .any(|f| dir.join(f).exists())
        }
        ProjectType::Node => dir.join("package.json").exists(),
        ProjectType::Rust => dir.join("Cargo.toml").exists(),
        ProjectType::Go => dir.join("go.mod").exists(),
        ProjectType::Docker => {
            dir.join("docker-compose.yml").exists() || dir.join("Dockerfile").exists()
        }
        ProjectType::Unknown => false,
    }
}

/// Suggest a default command based on project type and directory contents.
pub fn default_command_for_dir(pt: crate::model::ProjectType, dir: &Path) -> String {
    use crate::model::ProjectType;
    match pt {
        ProjectType::Python => detect_python_command(dir),
        ProjectType::Node => "npm run dev".into(),
        ProjectType::Rust => "cargo run".into(),
        ProjectType::Go => "go run .".into(),
        ProjectType::Docker => "docker compose up".into(),
        ProjectType::Unknown => "echo 'hello'".into(),
    }
}

/// Legacy wrapper without directory context.
pub fn default_command_for(pt: crate::model::ProjectType) -> String {
    default_command_for_dir(pt, Path::new("."))
}

/// Detect the correct Python start command by analyzing source files.
///
/// Checks for frameworks (FastAPI, Flask, Django) and entrypoint patterns
/// to generate the right command instead of always using `python main.py`.
fn detect_python_command(dir: &Path) -> String {
    // Check common entrypoint files
    let candidates = ["main.py", "app.py", "server.py", "run.py", "manage.py"];

    for filename in &candidates {
        let filepath = dir.join(filename);
        if !filepath.exists() {
            continue;
        }

        // Django: manage.py is always django
        if *filename == "manage.py" {
            return "python manage.py runserver".into();
        }

        // Read file content to detect framework
        let content = match fs::read_to_string(&filepath) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Check if it has uvicorn.run() in __main__ (self-starting)
        let has_main_block = content.contains("if __name__");
        let has_uvicorn_run = content.contains("uvicorn.run(");

        if has_main_block && has_uvicorn_run {
            return format!("python {}", filename);
        }

        // FastAPI / Starlette detection
        if content.contains("from fastapi") || content.contains("import fastapi")
            || content.contains("from starlette") {
            let module = filename.strip_suffix(".py").unwrap_or(filename);
            // Find the app variable name (usually `app = FastAPI(`)
            let app_var = detect_app_variable(&content, &["FastAPI(", "Starlette("]);
            return format!("uvicorn {}:{} --host 0.0.0.0 --port 8000", module, app_var);
        }

        // Flask detection
        if content.contains("from flask") || content.contains("import flask") {
            if has_main_block && content.contains(".run(") {
                return format!("python {}", filename);
            }
            let app_var = detect_app_variable(&content, &["Flask("]);
            return format!("flask --app {} run --port 5000", app_var);
        }

        // Generic python file with __main__
        if has_main_block {
            return format!("python {}", filename);
        }
    }

    // Fallback: check if there's any .py file
    "python main.py".into()
}

/// Find the variable name assigned to a framework constructor.
/// e.g., `app = FastAPI(` → "app", `application = Flask(` → "application"
fn detect_app_variable(content: &str, constructors: &[&str]) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        for constructor in constructors {
            if trimmed.contains(constructor) {
                // Pattern: `varname = Constructor(`
                if let Some(eq_pos) = trimmed.find('=') {
                    let var = trimmed[..eq_pos].trim();
                    if !var.is_empty() && var.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        return var.to_string();
                    }
                }
            }
        }
    }
    "app".into()
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

    #[test]
    fn test_detect_fastapi_uvicorn() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "fastapi\nuvicorn\n").unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            "from fastapi import FastAPI\n\napp = FastAPI()\n\n@app.get('/')\ndef root():\n    return {'ok': True}\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "uvicorn main:app --host 0.0.0.0 --port 8000");
    }

    #[test]
    fn test_detect_fastapi_custom_var() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from fastapi import FastAPI\n\nserver = FastAPI(title='My API')\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "uvicorn app:server --host 0.0.0.0 --port 8000");
    }

    #[test]
    fn test_detect_fastapi_self_starting() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            "from fastapi import FastAPI\nimport uvicorn\n\napp = FastAPI()\n\nif __name__ == '__main__':\n    uvicorn.run(app)\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        // Self-starting scripts should use `python main.py`
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn test_detect_flask() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from flask import Flask\n\napp = Flask(__name__)\n\n@app.route('/')\ndef index():\n    return 'hello'\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "flask --app app run --port 5000");
    }

    #[test]
    fn test_detect_flask_self_starting() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from flask import Flask\n\napp = Flask(__name__)\n\nif __name__ == '__main__':\n    app.run(port=5000)\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "python app.py");
    }

    #[test]
    fn test_detect_django() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("manage.py"), "#!/usr/bin/env python\nimport django\n").unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "python manage.py runserver");
    }

    #[test]
    fn test_detect_plain_main() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            "import sys\n\ndef main():\n    print('hello')\n\nif __name__ == '__main__':\n    main()\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn test_detect_app_variable_default() {
        let content = "# no constructor here\nprint('hello')\n";
        assert_eq!(detect_app_variable(content, &["FastAPI("]), "app");
    }

    #[test]
    fn test_detect_app_variable_custom() {
        let content = "from fastapi import FastAPI\n\nmy_api = FastAPI(title='test')\n";
        assert_eq!(detect_app_variable(content, &["FastAPI("]), "my_api");
    }
}
