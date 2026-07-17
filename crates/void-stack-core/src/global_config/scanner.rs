use std::fs;
use std::path::{Path, PathBuf};

use crate::model::ProjectType;

/// Directory names skipped during sub-project scanning. Build artifacts,
/// dependency caches and editor metadata never represent runnable services.
/// `deps` is included so we don't surface Phoenix/Elixir vendored deps
/// (which contain their own package.json/mix.exs) as bogus services.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "build",
    ".dart_tool",
    "__pycache__",
    "dist",
    ".next",
    "vendor",
    "deps",
    "_build",
    "venv",
    ".venv",
];

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
                if dir_name.starts_with('.') || SKIP_DIRS.contains(&dir_name.as_str()) {
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
                            if sub_name.starts_with('.') || SKIP_DIRS.contains(&sub_name.as_str()) {
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
        ProjectType::Elixir => dir.join("mix.exs").exists(),
        ProjectType::Unreal => crate::config::has_unreal_markers(dir),
        ProjectType::Unknown => false,
    }
}

/// Suggest a default command based on project type and directory contents.
pub fn default_command_for_dir(pt: ProjectType, dir: &Path) -> String {
    let cmd = match pt {
        ProjectType::Python => detect_python_command(dir),
        ProjectType::Node => {
            let pm = crate::docker::generate_dockerfile::detect_node_pkg_manager(dir);
            let script = detect_node_dev_script(dir);
            format!("{} run {}", pm, script)
        }
        ProjectType::Rust => "cargo run".into(),
        ProjectType::Go => detect_go_command(dir),
        ProjectType::Flutter => "flutter run".into(),
        ProjectType::Docker => "docker compose up".into(),
        ProjectType::Elixir => "mix phx.server".into(),
        // Unreal projects are opened through the editor, not a shell command;
        // surface a human-readable hint (same precedent as the Node guard).
        ProjectType::Unreal => "echo 'Open this project in Unreal Editor / UEFN'".into(),
        ProjectType::Unknown => "echo 'hello'".into(),
    };

    // Guard: Unity/Godot/terrain projects sometimes hit this with no
    // package.json — `npm run dev` would fail loudly. Replace with a
    // human-readable hint so the user knows to configure it.
    let needs_pkg_json =
        cmd.starts_with("npm ") || cmd.starts_with("pnpm ") || cmd.starts_with("yarn ");
    if needs_pkg_json && !dir.join("package.json").exists() {
        return "echo 'No start command detected — configure manually'".into();
    }

    // WSL/Linux paths invoked from Windows need a `wsl.exe --exec` prefix
    // for npm/pnpm/yarn/mix to be resolvable inside the distro.
    if is_wsl_path(dir) && (needs_pkg_json || cmd.starts_with("mix ")) {
        return format!("wsl.exe --exec {}", cmd);
    }

    cmd
}

/// Legacy wrapper without directory context.
pub fn default_command_for(pt: ProjectType) -> String {
    default_command_for_dir(pt, Path::new("."))
}

/// Pick the most useful npm script from package.json. Falls back to "dev"
/// when nothing else is configured, so `npm run dev` still appears (and the
/// project owner sees a hint that the script is missing).
pub(crate) fn detect_node_dev_script(dir: &Path) -> &'static str {
    let content = match fs::read_to_string(dir.join("package.json")) {
        Ok(c) => c,
        Err(_) => return "dev",
    };
    let pkg: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return "dev",
    };
    let scripts = match pkg.get("scripts").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return "dev",
    };
    for candidate in ["dev", "start", "serve", "develop"] {
        if scripts.contains_key(candidate) {
            return candidate;
        }
    }
    "dev"
}

/// WSL-ness detection: UNC paths (`\\wsl$`, `\\wsl.localhost`) and the
/// Linux-side paths that show up when running under WSL itself.
pub(crate) fn is_wsl_path(dir: &Path) -> bool {
    let s = dir.to_string_lossy();
    s.starts_with(r"\\wsl")
        || s.starts_with("//wsl")
        || s.starts_with("/home")
        || s.starts_with("/mnt/")
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
    results.sort_by_key(|x| x.1.clone());
    results
}

// NOTE: `scan_wsl_subprojects` is intentionally not tested here — it shells
// out to `wsl.exe`, so its result depends on the host machine.
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Create a file with the given content, creating parent dirs as needed.
    fn write_file(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    // ── scan_subprojects ──

    #[test]
    fn test_scan_subprojects_includes_root_with_entrypoint() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("requirements.txt"), "flask\n");
        write_file(&dir.path().join("app.py"), "print('hi')\n");

        let results = scan_subprojects(dir.path());
        assert_eq!(results.len(), 1, "only the root project should be found");
        assert_eq!(
            results[0].2,
            ProjectType::Python,
            "root should be detected as Python"
        );
        assert_eq!(
            results[0].1,
            dir.path(),
            "root entry should point at the scanned directory"
        );
    }

    #[test]
    fn test_scan_subprojects_skips_root_without_entrypoint() {
        // setup.py alone marks the dir as Python but has_entrypoint rejects it
        // (no runnable file, no pyproject.toml, no requirements.txt).
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("setup.py"),
            "from setuptools import setup\n",
        );

        let results = scan_subprojects(dir.path());
        assert!(
            results.is_empty(),
            "library-only root should not be listed as a service: {results:?}"
        );
    }

    #[test]
    fn test_scan_subprojects_detects_multiple_subdirs() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("api").join("Cargo.toml"),
            "[package]\nname = \"api\"\n",
        );
        write_file(
            &dir.path().join("web").join("package.json"),
            r#"{"name":"web"}"#,
        );

        let mut results = scan_subprojects(dir.path());
        results.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(
            results.len(),
            2,
            "both subprojects should be found: {results:?}"
        );
        assert_eq!(results[0].0, "api");
        assert_eq!(results[0].2, ProjectType::Rust, "api should be Rust");
        assert_eq!(results[1].0, "web");
        assert_eq!(results[1].2, ProjectType::Node, "web should be Node");
    }

    #[test]
    fn test_scan_subprojects_skips_hidden_and_skip_dirs() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("node_modules").join("package.json"),
            r#"{"name":"dep"}"#,
        );
        write_file(
            &dir.path().join(".git").join("package.json"),
            r#"{"name":"x"}"#,
        );
        write_file(
            &dir.path().join("deps").join("phoenix").join("mix.exs"),
            "defmodule Phoenix.MixProject do end\n",
        );

        let results = scan_subprojects(dir.path());
        assert!(
            results.is_empty(),
            "node_modules, hidden dirs and deps must be skipped: {results:?}"
        );
    }

    #[test]
    fn test_scan_subprojects_finds_nested_one_level_deeper() {
        // backends/ has no marker itself, but backends/tts does
        let dir = tempdir().unwrap();
        write_file(
            &dir.path()
                .join("backends")
                .join("tts")
                .join("requirements.txt"),
            "torch\n",
        );

        let results = scan_subprojects(dir.path());
        assert_eq!(
            results.len(),
            1,
            "nested subproject should be found: {results:?}"
        );
        assert_eq!(
            results[0].0, "backends/tts",
            "nested name should be parent/child"
        );
        assert_eq!(results[0].2, ProjectType::Python);
    }

    #[test]
    fn test_scan_subprojects_expands_go_air_services() {
        let dir = tempdir().unwrap();
        let go_dir = dir.path().join("gosvc");
        write_file(
            &go_dir.join("go.mod"),
            "module example.com/gosvc\n\ngo 1.22\n",
        );
        write_file(&go_dir.join("api").join(".air.toml"), "");
        write_file(&go_dir.join("worker").join(".air.toml"), "");

        let mut results = scan_subprojects(dir.path());
        results.sort_by(|a, b| a.0.cmp(&b.0));

        let names: Vec<&str> = results.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(
            names,
            vec!["gosvc/api", "gosvc/worker"],
            "each .air.toml should become its own Go service"
        );
        assert!(
            results.iter().all(|r| r.2 == ProjectType::Go),
            "air services should all be Go"
        );
    }

    // ── has_entrypoint ──

    #[test]
    fn test_has_entrypoint_python_requires_runnable_file_or_manifest() {
        let dir = tempdir().unwrap();
        assert!(
            !has_entrypoint(ProjectType::Python, dir.path()),
            "empty dir has no Python entrypoint"
        );

        write_file(&dir.path().join("main.py"), "print('hi')\n");
        assert!(
            has_entrypoint(ProjectType::Python, dir.path()),
            "main.py should count as an entrypoint"
        );
    }

    #[test]
    fn test_has_entrypoint_per_project_type() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("package.json"), "{}");
        write_file(&dir.path().join("Cargo.toml"), "[package]\n");
        write_file(&dir.path().join("go.mod"), "module x\n");
        write_file(&dir.path().join("Dockerfile"), "FROM alpine\n");

        assert!(
            has_entrypoint(ProjectType::Node, dir.path()),
            "package.json"
        );
        assert!(has_entrypoint(ProjectType::Rust, dir.path()), "Cargo.toml");
        assert!(has_entrypoint(ProjectType::Go, dir.path()), "go.mod");
        assert!(
            has_entrypoint(ProjectType::Docker, dir.path()),
            "Dockerfile"
        );
        assert!(
            !has_entrypoint(ProjectType::Flutter, dir.path()),
            "no pubspec.yaml means no Flutter entrypoint"
        );
        assert!(
            !has_entrypoint(ProjectType::Unknown, dir.path()),
            "Unknown never has an entrypoint"
        );
    }

    #[test]
    fn test_has_entrypoint_unreal_requires_markers() {
        let dir = tempdir().unwrap();
        assert!(
            !has_entrypoint(ProjectType::Unreal, dir.path()),
            "empty dir has no Unreal markers"
        );

        write_file(&dir.path().join("MyGame.uproject"), "{}");
        assert!(
            has_entrypoint(ProjectType::Unreal, dir.path()),
            ".uproject should count as an Unreal entrypoint"
        );
    }

    #[test]
    fn test_has_entrypoint_unreal_verse_file() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("device.verse"),
            "using { /Fortnite.com/Devices }\n",
        );
        assert!(
            has_entrypoint(ProjectType::Unreal, dir.path()),
            "a top-level .verse file should count as an Unreal entrypoint"
        );
    }

    // ── default_command_for_dir ──

    #[test]
    fn test_default_command_node_uses_detected_script() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("package.json"),
            r#"{"scripts":{"start":"node index.js"}}"#,
        );

        assert_eq!(
            default_command_for_dir(ProjectType::Node, dir.path()),
            "npm run start",
            "should pick the start script when dev is absent"
        );
    }

    #[test]
    fn test_default_command_node_without_package_json_yields_hint() {
        let dir = tempdir().unwrap();

        let cmd = default_command_for_dir(ProjectType::Node, dir.path());
        assert!(
            cmd.starts_with("echo"),
            "missing package.json should produce a human-readable hint, got: {cmd}"
        );
    }

    #[test]
    fn test_default_command_static_project_types() {
        let dir = tempdir().unwrap();
        assert_eq!(
            default_command_for_dir(ProjectType::Rust, dir.path()),
            "cargo run"
        );
        assert_eq!(
            default_command_for_dir(ProjectType::Docker, dir.path()),
            "docker compose up"
        );
        assert_eq!(
            default_command_for_dir(ProjectType::Flutter, dir.path()),
            "flutter run"
        );
    }

    #[test]
    fn test_default_command_unreal_yields_editor_hint() {
        let dir = tempdir().unwrap();
        let cmd = default_command_for_dir(ProjectType::Unreal, dir.path());
        assert!(
            cmd.starts_with("echo"),
            "Unreal projects should get a human-readable hint, got: {cmd}"
        );
        assert!(
            cmd.contains("Unreal Editor / UEFN"),
            "hint should mention the editor, got: {cmd}"
        );
    }

    #[test]
    fn test_default_command_go_prefers_air_when_configured() {
        let dir = tempdir().unwrap();
        assert_eq!(
            default_command_for_dir(ProjectType::Go, dir.path()),
            "go run .",
            "plain Go project should use go run"
        );

        write_file(&dir.path().join(".air.toml"), "");
        assert_eq!(
            default_command_for_dir(ProjectType::Go, dir.path()),
            "air",
            ".air.toml should switch the command to air"
        );
    }

    // ── detect_node_dev_script ──

    #[test]
    fn test_detect_node_dev_script_priority_order() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("package.json"),
            r#"{"scripts":{"serve":"x","dev":"y","start":"z"}}"#,
        );

        assert_eq!(
            detect_node_dev_script(dir.path()),
            "dev",
            "dev should win over start and serve"
        );
    }

    #[test]
    fn test_detect_node_dev_script_falls_back_on_invalid_json() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("package.json"), "not json at all");

        assert_eq!(
            detect_node_dev_script(dir.path()),
            "dev",
            "unparseable package.json should fall back to dev"
        );
    }

    #[test]
    fn test_detect_node_dev_script_falls_back_without_scripts() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("package.json"), r#"{"name":"x"}"#);

        assert_eq!(
            detect_node_dev_script(dir.path()),
            "dev",
            "missing scripts object should fall back to dev"
        );
    }

    // ── is_wsl_path ──

    #[test]
    fn test_is_wsl_path_recognizes_wsl_locations() {
        assert!(is_wsl_path(Path::new(r"\\wsl$\Ubuntu\home\user\proj")));
        assert!(is_wsl_path(Path::new("//wsl.localhost/Ubuntu/home/user")));
        assert!(is_wsl_path(Path::new("/home/user/proj")));
        assert!(is_wsl_path(Path::new("/mnt/c/workspace")));
    }

    #[test]
    fn test_is_wsl_path_rejects_windows_paths() {
        assert!(!is_wsl_path(Path::new(r"C:\workspace\proj")));
        assert!(!is_wsl_path(Path::new(r"F:\workspace\devlaunch-rs")));
    }

    // ── scan_air_services ──

    #[test]
    fn test_scan_air_services_empty_when_no_air_toml() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("go.mod"), "module x\n");

        assert!(
            scan_air_services(dir.path(), "svc").is_empty(),
            "no .air.toml anywhere should return no services"
        );
    }

    #[test]
    fn test_scan_air_services_includes_root_when_it_also_has_air() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join(".air.toml"), "");
        write_file(&dir.path().join("api").join(".air.toml"), "");

        let results = scan_air_services(dir.path(), "svc");
        let names: Vec<&str> = results.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(
            names,
            vec!["svc", "svc/api"],
            "root air service should be inserted first"
        );
    }

    #[test]
    fn test_scan_air_services_finds_two_levels_deep() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("cmd").join("api").join(".air.toml"), "");

        let results = scan_air_services(dir.path(), "svc");
        assert_eq!(
            results.len(),
            1,
            "nested .air.toml should be found: {results:?}"
        );
        assert_eq!(results[0].0, "svc/cmd/api");
    }

    #[test]
    fn test_scan_air_services_skips_vendor() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("vendor").join(".air.toml"), "");

        assert!(
            scan_air_services(dir.path(), "svc").is_empty(),
            "vendor directory must be ignored"
        );
    }

    // ── detect_python_command ──

    #[test]
    fn test_detect_python_command_django_manage_py() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("manage.py"), "#!/usr/bin/env python\n");

        assert_eq!(
            detect_python_command(dir.path()),
            "python manage.py runserver",
            "manage.py should always mean Django"
        );
    }

    #[test]
    fn test_detect_python_command_fastapi_with_custom_app_var() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("main.py"),
            "from fastapi import FastAPI\n\napi = FastAPI()\n",
        );

        assert_eq!(
            detect_python_command(dir.path()),
            "uvicorn main:api --host 0.0.0.0 --port 8000",
            "should use the detected app variable name"
        );
    }

    #[test]
    fn test_detect_python_command_self_starting_uvicorn() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("main.py"),
            "import uvicorn\nfrom fastapi import FastAPI\napp = FastAPI()\n\nif __name__ == \"__main__\":\n    uvicorn.run(app)\n",
        );

        assert_eq!(
            detect_python_command(dir.path()),
            "python main.py",
            "self-starting uvicorn.run should be launched directly"
        );
    }

    #[test]
    fn test_detect_python_command_flask_without_main_block() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("app.py"),
            "from flask import Flask\n\napp = Flask(__name__)\n",
        );

        assert_eq!(
            detect_python_command(dir.path()),
            "flask --app app run --port 5000",
            "Flask without __main__ should use the flask CLI"
        );
    }

    #[test]
    fn test_detect_python_command_flask_with_main_block() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("app.py"),
            "from flask import Flask\napp = Flask(__name__)\n\nif __name__ == \"__main__\":\n    app.run()\n",
        );

        assert_eq!(
            detect_python_command(dir.path()),
            "python app.py",
            "self-running Flask app should be launched directly"
        );
    }

    #[test]
    fn test_detect_python_command_generic_script_with_main_block() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("run.py"),
            "if __name__ == \"__main__\":\n    print(\"hi\")\n",
        );

        assert_eq!(
            detect_python_command(dir.path()),
            "python run.py",
            "generic script with __main__ should be launched directly"
        );
    }

    #[test]
    fn test_detect_python_command_fallback() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_python_command(dir.path()),
            "python main.py",
            "empty dir should fall back to python main.py"
        );
    }

    // ── detect_app_variable ──

    #[test]
    fn test_detect_app_variable_custom_name() {
        let content = "from flask import Flask\napplication = Flask(__name__)\n";
        assert_eq!(
            detect_app_variable(content, &["Flask("]),
            "application",
            "assigned variable name should be extracted"
        );
    }

    #[test]
    fn test_detect_app_variable_defaults_to_app() {
        assert_eq!(
            detect_app_variable("print('nothing here')", &["FastAPI("]),
            "app",
            "no constructor match should default to app"
        );
    }
}
