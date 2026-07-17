use serde::Serialize;
use tauri::State;

use std::path::Path;

use void_stack_core::config::detect_project_type;
use void_stack_core::global_config::{
    default_command_for_dir, load_global_config, save_global_config, scan_subprojects,
};
use void_stack_core::model::Target;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
    pub project_type: String,
    pub services: Vec<ServiceInfo>,
}

#[derive(Serialize)]
pub struct ServiceInfo {
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
    pub target: String,
    pub tech: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_ports: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_volumes: Option<Vec<String>>,
}

fn project_to_info(p: &void_stack_core::model::Project) -> ProjectInfo {
    ProjectInfo {
        name: p.name.clone(),
        path: p.path.clone(),
        project_type: p
            .project_type
            .map(|pt| format!("{:?}", pt))
            .unwrap_or_else(|| "Unknown".to_string()),
        services: p
            .services
            .iter()
            .map(|s| {
                let tech = detect_service_tech(s);
                ServiceInfo {
                    name: s.name.clone(),
                    command: s.command.clone(),
                    working_dir: s.working_dir.clone(),
                    target: format!("{:?}", s.target),
                    tech,
                    docker_ports: s
                        .docker
                        .as_ref()
                        .map(|d| d.ports.clone())
                        .filter(|p| !p.is_empty()),
                    docker_volumes: s
                        .docker
                        .as_ref()
                        .map(|d| d.volumes.clone())
                        .filter(|v| !v.is_empty()),
                }
            })
            .collect(),
    }
}

/// Command-prefix → technology table used as a fallback when the
/// working_dir does not identify the project type. Checked in order.
const COMMAND_TECH_TABLE: &[(&[&str], &str)] = &[
    (
        &["python", "uvicorn", "flask", "gunicorn", "django"],
        "python",
    ),
    (
        &["npm", "node", "yarn", "pnpm", "bun", "vite", "next", "nuxt"],
        "node",
    ),
    (&["cargo", "rustc"], "rust"),
    (&["go ", "go."], "go"),
    (&["flutter", "dart"], "flutter"),
    (&["java", "mvn", "gradle"], "java"),
    (&["dotnet"], "dotnet"),
    (&["php", "artisan", "composer"], "php"),
];

/// Detect the technology of a service from its working_dir and command.
fn detect_service_tech(service: &void_stack_core::model::Service) -> String {
    // Docker target → check command for hints, default to "docker"
    if service.target == Target::Docker {
        let cmd = service.command.to_lowercase();
        if cmd.contains("docker compose") {
            return "docker-compose".into();
        }
        return "docker".into();
    }

    // Detect from working_dir first (most reliable)
    if let Some(tech) = tech_from_working_dir(service.working_dir.as_deref()) {
        return tech;
    }

    // Fallback: detect from command
    tech_from_command(&service.command.to_lowercase())
}

/// Detect the technology from the service's working directory, if any.
fn tech_from_working_dir(dir: Option<&str>) -> Option<String> {
    let path = std::path::Path::new(dir?);
    let pt = detect_project_type(path);
    if pt != void_stack_core::model::ProjectType::Unknown {
        return Some(format!("{:?}", pt).to_lowercase());
    }
    None
}

/// Detect the technology from a lowercased command via the prefix table.
fn tech_from_command(cmd: &str) -> String {
    for (prefixes, tech) in COMMAND_TECH_TABLE {
        if prefixes.iter().any(|p| cmd.starts_with(p)) {
            return (*tech).into();
        }
    }
    "unknown".into()
}

#[tauri::command]
pub fn list_projects() -> Result<Vec<ProjectInfo>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    Ok(config.projects.iter().map(project_to_info).collect())
}

#[tauri::command]
pub fn add_project(name: String, path: String, wsl: Option<bool>) -> Result<ProjectInfo, String> {
    use void_stack_core::runner::local::is_wsl_unc_path;

    let mut config = load_global_config().map_err(|e| e.to_string())?;

    if config
        .projects
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(&name))
    {
        return Err(format!("El proyecto '{}' ya existe", name));
    }

    // Detect WSL from UNC path or explicit flag
    let is_wsl = wsl.unwrap_or(false) || is_wsl_unc_path(&path);
    let target = if is_wsl {
        Target::Wsl
    } else {
        Target::native()
    };

    // For WSL UNC paths, std::fs works directly — use as-is for scanning
    // For Windows paths, strip the \\?\ prefix
    let scan_path = if is_wsl {
        path.clone()
    } else {
        strip_win_prefix(&path)
    };

    let scan_dir = Path::new(&scan_path);
    let sub_services = scan_subprojects(scan_dir);

    let services: Vec<void_stack_core::model::Service> = if sub_services.is_empty() {
        let pt = void_stack_core::config::detect_project_type(scan_dir);
        let cmd = default_command_for_dir(pt, scan_dir);
        vec![void_stack_core::model::Service {
            name: name.clone(),
            command: cmd,
            target,
            working_dir: Some(path.clone()),
            enabled: true,
            env_vars: Vec::new(),
            depends_on: Vec::new(),
            docker: None,
        }]
    } else {
        sub_services
            .into_iter()
            .map(|(svc_name, svc_path, svc_type)| {
                let cmd = default_command_for_dir(svc_type, &svc_path);
                void_stack_core::model::Service {
                    name: svc_name,
                    command: cmd,
                    target,
                    working_dir: Some(svc_path.to_string_lossy().to_string()),
                    enabled: true,
                    env_vars: Vec::new(),
                    depends_on: Vec::new(),
                    docker: None,
                }
            })
            .collect()
    };

    let project = void_stack_core::model::Project {
        name: name.clone(),
        description: String::new(),
        path: path.clone(),
        project_type: Some(void_stack_core::model::ProjectType::Unknown),
        tags: Vec::new(),
        services,
        hooks: None,
    };

    let info = project_to_info(&project);
    config.projects.push(project);
    save_global_config(&config).map_err(|e| e.to_string())?;

    Ok(info)
}

#[tauri::command]
pub fn browse_directory(path: String) -> Result<Vec<BrowseEntry>, String> {
    let entries = std::fs::read_dir(&path).map_err(|e| format!("Cannot read {}: {}", path, e))?;

    let mut result = Vec::new();
    for entry in entries.flatten() {
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden dirs
        if name.starts_with('.') {
            continue;
        }
        result.push(BrowseEntry {
            name,
            path: entry.path().to_string_lossy().to_string(),
        });
    }
    result.sort_by_key(|d| d.name.to_lowercase());
    Ok(result)
}

#[derive(serde::Serialize)]
pub struct BrowseEntry {
    pub name: String,
    pub path: String,
}

#[tauri::command]
pub fn list_wsl_distros() -> Result<Vec<String>, String> {
    let output = std::process::Command::new("wsl")
        .args(["--list", "--quiet"])
        .output()
        .map_err(|e| format!("WSL not available: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    // wsl output is UTF-16LE on Windows
    let text = String::from_utf16_lossy(
        &output
            .stdout
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<u16>>(),
    );

    let distros: Vec<String> = text
        .lines()
        .map(|l| l.trim().trim_matches('\0').to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(distros)
}

/// Rename and/or move a registered project preserving all derived data
/// (semantic index, structural graph, trust approval, git hook). Returns
/// the updated project info plus the migration log for the UI.
#[tauri::command]
pub async fn update_project_cmd(
    name: String,
    new_name: Option<String>,
    new_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<(ProjectInfo, Vec<String>), String> {
    if new_name.is_none() && new_path.is_none() {
        return Err("Nothing to change — pass a new name and/or path".to_string());
    }

    let config = load_global_config().map_err(|e| e.to_string())?;
    let old_project = void_stack_core::global_config::find_project(&config, &name).cloned();

    let (updated, log) = void_stack_core::global_config::update_project(
        &name,
        new_name.as_deref(),
        new_path.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    // Re-key the in-memory caches that are keyed by name/path.
    {
        let mut managers = state.managers.lock().await;
        if let Some(mgr) = managers.remove(&name) {
            managers.insert(updated.name.clone(), mgr);
        }
    }
    if let Some(old) = &old_project
        && void_stack_core::vector_index::is_watching(old)
    {
        void_stack_core::vector_index::unwatch_project(old);
        let _ = void_stack_core::vector_index::watch_project(&updated);
    }

    Ok((project_to_info(&updated), log))
}

#[tauri::command]
pub async fn remove_project_cmd(name: String, state: State<'_, AppState>) -> Result<bool, String> {
    let mut config = load_global_config().map_err(|e| e.to_string())?;

    // Stop services if running
    if let Some(project) = void_stack_core::global_config::find_project(&config, &name).cloned() {
        let mgr = state.get_manager(&project).await;
        let _ = mgr.stop_all().await;
    }

    // Remove from managers cache
    {
        let mut managers = state.managers.lock().await;
        managers.remove(&name);
    }

    void_stack_core::global_config::remove_project(&mut config, &name);
    save_global_config(&config).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support;
    use void_stack_core::model::Service;

    /// Build a service with the given command, working_dir and target.
    fn svc(command: &str, working_dir: Option<&str>, target: Target) -> Service {
        Service {
            name: "s".to_string(),
            command: command.to_string(),
            target,
            working_dir: working_dir.map(|d| d.to_string()),
            enabled: true,
            env_vars: Vec::new(),
            depends_on: Vec::new(),
            docker: None,
        }
    }

    #[test]
    fn test_tech_from_command_table_order_and_prefixes() {
        assert_eq!(tech_from_command("uvicorn main:app"), "python");
        assert_eq!(tech_from_command("python -m app"), "python");
        assert_eq!(tech_from_command("npm run dev"), "node");
        assert_eq!(tech_from_command("next dev"), "node");
        assert_eq!(tech_from_command("cargo run"), "rust");
        assert_eq!(tech_from_command("go run ."), "go");
        assert_eq!(tech_from_command("go.mod"), "go");
        assert_eq!(tech_from_command("flutter run"), "flutter");
        assert_eq!(tech_from_command("mvn spring-boot:run"), "java");
        assert_eq!(tech_from_command("dotnet run"), "dotnet");
        assert_eq!(tech_from_command("php artisan serve"), "php");
        assert_eq!(tech_from_command("artisan serve"), "php");
        assert_eq!(tech_from_command("some-unknown-binary"), "unknown");
    }

    #[test]
    fn test_tech_from_working_dir_detects_and_falls_through() {
        // A directory with Cargo.toml is detected as Rust.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let detected = tech_from_working_dir(Some(&dir.path().to_string_lossy()));
        assert_eq!(detected.as_deref(), Some("rust"));

        // An empty directory yields no detection.
        let empty = tempfile::tempdir().unwrap();
        assert!(tech_from_working_dir(Some(&empty.path().to_string_lossy())).is_none());

        // No working_dir at all.
        assert!(tech_from_working_dir(None).is_none());
    }

    #[test]
    fn test_detect_service_tech_docker_target() {
        assert_eq!(
            detect_service_tech(&svc("docker compose up", None, Target::Docker)),
            "docker-compose"
        );
        assert_eq!(
            detect_service_tech(&svc("docker run nginx", None, Target::Docker)),
            "docker"
        );
    }

    #[test]
    fn test_detect_service_tech_prefers_working_dir_over_command() {
        // working_dir says rust (Cargo.toml) even though the command looks like node.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let s = svc(
            "npm run dev",
            Some(&dir.path().to_string_lossy()),
            Target::native(),
        );
        assert_eq!(detect_service_tech(&s), "rust");
    }

    #[test]
    fn test_detect_service_tech_command_fallback() {
        // No usable working_dir → falls back to command inspection.
        let s = svc("uvicorn app:main", None, Target::native());
        assert_eq!(detect_service_tech(&s), "python");
    }

    #[test]
    fn test_project_to_info_maps_services_and_docker() {
        let mut project = test_support::project("Demo", std::path::Path::new("C:/tmp/demo"));
        project.project_type = Some(void_stack_core::model::ProjectType::Node);
        project.services.push(Service {
            name: "web".to_string(),
            command: "docker compose up".to_string(),
            target: Target::Docker,
            working_dir: Some("C:/tmp/demo".to_string()),
            enabled: true,
            env_vars: Vec::new(),
            depends_on: Vec::new(),
            docker: Some(void_stack_core::model::DockerConfig {
                ports: vec!["8080:80".to_string()],
                volumes: vec![],
                extra_args: vec![],
            }),
        });

        let info = project_to_info(&project);
        assert_eq!(info.name, "Demo");
        assert_eq!(info.project_type, "Node");
        assert_eq!(info.services.len(), 1);
        let s = &info.services[0];
        assert_eq!(s.tech, "docker-compose");
        assert_eq!(s.target, "Docker");
        assert_eq!(
            s.docker_ports.as_deref(),
            Some(&["8080:80".to_string()][..])
        );
        // Empty volumes are filtered to None.
        assert!(s.docker_volumes.is_none());
    }

    #[test]
    fn test_project_to_info_unknown_type() {
        let project = test_support::project("NoType", std::path::Path::new("C:/tmp/x"));
        let info = project_to_info(&project);
        assert_eq!(info.project_type, "Unknown");
    }

    #[test]
    fn test_list_and_add_project_roundtrip() {
        let _g = test_support::config_guard();

        // Fresh isolated config lists nothing.
        assert!(list_projects().unwrap().is_empty());

        // Register via add_project against a Rust source dir.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let path = dir.path().to_string_lossy().to_string();

        let info = add_project("MyProj".to_string(), path.clone(), None).unwrap();
        assert_eq!(info.name, "MyProj");
        assert!(!info.services.is_empty());

        // It is now listed.
        let listed = list_projects().unwrap();
        assert!(listed.iter().any(|p| p.name == "MyProj"));

        // Duplicate name (case-insensitive) is rejected.
        let dup = add_project("myproj".to_string(), path, None);
        assert!(dup.is_err());
    }

    #[test]
    fn test_browse_directory_lists_visible_dirs_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("beta")).unwrap();
        std::fs::create_dir(dir.path().join("alpha")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join("file.txt"), "x").unwrap();

        let entries = browse_directory(dir.path().to_string_lossy().to_string()).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        // Sorted, no hidden dir, no files.
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_browse_directory_missing_path_errors() {
        assert!(browse_directory("Z:/no/such/dir/anywhere".to_string()).is_err());
    }
}
