use std::fs;
use std::path::{Path, PathBuf};

use crate::model::ProjectType;

/// Scan a directory for sub-projects (monorepo detection).
/// Returns a list of (subdir_name, path, detected_type).
pub fn scan_subprojects(root: &Path) -> Vec<(String, PathBuf, ProjectType)> {
    use crate::config::detect_project_type;

    let mut results = Vec::new();

    // Check root itself -- only add if it has a runnable entrypoint
    let root_type = detect_project_type(root);
    if root_type != ProjectType::Unknown && has_entrypoint(root_type, root) {
        let name = root
            .file_name()
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
                if sub_type != ProjectType::Unknown {
                    // For Go projects with .air.toml in subdirs, detect each air service
                    if sub_type == ProjectType::Go {
                        let air_services = scan_air_services(&path, &dir_name);
                        if !air_services.is_empty() {
                            results.extend(air_services);
                        } else {
                            results.push((dir_name.clone(), path.clone(), sub_type));
                        }
                    } else {
                        results.push((dir_name.clone(), path.clone(), sub_type));
                    }
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
                            if deep_type != ProjectType::Unknown {
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
pub(crate) fn has_entrypoint(pt: ProjectType, dir: &Path) -> bool {
    match pt {
        ProjectType::Python => {
            // Must have an executable Python file or a project manifest
            ["main.py", "app.py", "server.py", "run.py", "manage.py"]
                .iter()
                .any(|f| dir.join(f).exists())
                || dir.join("pyproject.toml").exists()
                || dir.join("requirements.txt").exists()
        }
        ProjectType::Node => dir.join("package.json").exists(),
        ProjectType::Rust => dir.join("Cargo.toml").exists(),
        ProjectType::Go => dir.join("go.mod").exists(),
        ProjectType::Flutter => dir.join("pubspec.yaml").exists(),
        ProjectType::Docker => {
            dir.join("docker-compose.yml").exists() || dir.join("Dockerfile").exists()
        }
        ProjectType::Unknown => false,
    }
}

/// Suggest a default command based on project type and directory contents.
pub fn default_command_for_dir(pt: ProjectType, dir: &Path) -> String {
    match pt {
        ProjectType::Python => detect_python_command(dir),
        ProjectType::Node => "npm run dev".into(),
        ProjectType::Rust => "cargo run".into(),
        ProjectType::Go => detect_go_command(dir),
        ProjectType::Flutter => "flutter run".into(),
        ProjectType::Docker => "docker compose up".into(),
        ProjectType::Unknown => "echo 'hello'".into(),
    }
}

/// Legacy wrapper without directory context.
pub fn default_command_for(pt: ProjectType) -> String {
    default_command_for_dir(pt, Path::new("."))
}

/// Detect the correct Python start command by analyzing source files.
///
/// Checks for frameworks (FastAPI, Flask, Django) and entrypoint patterns
/// to generate the right command instead of always using `python main.py`.
/// Scan a Go project for multiple `.air.toml` files in subdirectories.
/// Returns one service per `.air.toml` found, or empty if none/only root.
pub(crate) fn scan_air_services(
    go_root: &Path,
    parent_name: &str,
) -> Vec<(String, PathBuf, ProjectType)> {
    let mut results = Vec::new();

    // Check root .air.toml -- if only root has it, let the normal flow handle it
    let root_has_air = go_root.join(".air.toml").exists();

    // Scan up to 2 levels deep for .air.toml
    if let Ok(entries) = fs::read_dir(go_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "vendor" {
                continue;
            }

            if path.join(".air.toml").exists() {
                results.push((
                    format!("{}/{}", parent_name, name),
                    path.clone(),
                    ProjectType::Go,
                ));
            }

            // One level deeper (e.g. cmd/api/.air.toml)
            if let Ok(sub_entries) = fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    let sub_path = sub_entry.path();
                    if sub_path.is_dir() && sub_path.join(".air.toml").exists() {
                        let sub_name = sub_entry.file_name().to_string_lossy().to_string();
                        results.push((
                            format!("{}/{}/{}", parent_name, name, sub_name),
                            sub_path,
                            ProjectType::Go,
                        ));
                    }
                }
            }
        }
    }

    // If we found air services in subdirs but root also has one, add root too
    if !results.is_empty() && root_has_air {
        results.insert(
            0,
            (
                parent_name.to_string(),
                go_root.to_path_buf(),
                ProjectType::Go,
            ),
        );
    }

    results
}

/// Detect the best command for a Go project.
/// If `.air.toml` exists, use `air` for hot-reload. Otherwise `go run .`.
pub(crate) fn detect_go_command(dir: &Path) -> String {
    if dir.join(".air.toml").exists() {
        "air".into()
    } else {
        "go run .".into()
    }
}

pub(crate) fn detect_python_command(dir: &Path) -> String {
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
        if content.contains("from fastapi")
            || content.contains("import fastapi")
            || content.contains("from starlette")
        {
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
/// e.g., `app = FastAPI(` -> "app", `application = Flask(` -> "application"
pub(crate) fn detect_app_variable(content: &str, constructors: &[&str]) -> String {
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
pub fn scan_wsl_subprojects(wsl_path: &str) -> Vec<(String, String, ProjectType)> {
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

    use crate::process_util::HideWindow;
    let output = std::process::Command::new("wsl.exe")
        .args(["-e", "bash", "-c", &script])
        .hide_window()
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
        } else if files
            .iter()
            .any(|f| f == "requirements.txt" || f == "pyproject.toml" || f == "setup.py")
        {
            ProjectType::Python
        } else if files.iter().any(|f| f == "package.json") {
            ProjectType::Node
        } else if files.iter().any(|f| f == "go.mod") {
            ProjectType::Go
        } else if files
            .iter()
            .any(|f| f == "docker-compose.yml" || f == "Dockerfile")
        {
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
