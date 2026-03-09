use serde::Serialize;
use tauri::State;

use std::path::Path;

use void_stack_core::global_config::{
    load_global_config, save_global_config, scan_subprojects, default_command_for_dir,
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
}

fn project_to_info(p: &void_stack_core::model::Project) -> ProjectInfo {
    ProjectInfo {
        name: p.name.clone(),
        path: p.path.clone(),
        project_type: p.project_type.map(|pt| format!("{:?}", pt)).unwrap_or_else(|| "Unknown".to_string()),
        services: p.services.iter().map(|s| ServiceInfo {
            name: s.name.clone(),
            command: s.command.clone(),
            working_dir: s.working_dir.clone(),
            target: format!("{:?}", s.target),
        }).collect(),
    }
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

    if config.projects.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
        return Err(format!("El proyecto '{}' ya existe", name));
    }

    // Detect WSL from UNC path or explicit flag
    let is_wsl = wsl.unwrap_or(false) || is_wsl_unc_path(&path);
    let target = if is_wsl { Target::Wsl } else { Target::Windows };

    // For WSL UNC paths, std::fs works directly — use as-is for scanning
    // For Windows paths, strip the \\?\ prefix
    let scan_path = if is_wsl { path.clone() } else { strip_win_prefix(&path) };

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
        }]
    } else {
        sub_services.into_iter().map(|(svc_name, svc_path, svc_type)| {
            let cmd = default_command_for_dir(svc_type, &svc_path);
            void_stack_core::model::Service {
                name: svc_name,
                command: cmd,
                target,
                working_dir: Some(svc_path.to_string_lossy().to_string()),
                enabled: true,
                env_vars: Vec::new(),
                depends_on: Vec::new(),
            }
        }).collect()
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
    let entries = std::fs::read_dir(&path)
        .map_err(|e| format!("Cannot read {}: {}", path, e))?;

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
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
