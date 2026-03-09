use serde::Serialize;
use tauri::State;

use std::path::Path;

use devlaunch_core::global_config::{
    load_global_config, save_global_config, scan_subprojects, default_command_for_dir,
};
use devlaunch_core::model::Target;
use devlaunch_core::runner::local::strip_win_prefix;

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

fn project_to_info(p: &devlaunch_core::model::Project) -> ProjectInfo {
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
    let mut config = load_global_config().map_err(|e| e.to_string())?;

    if config.projects.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
        return Err(format!("El proyecto '{}' ya existe", name));
    }

    let is_wsl = wsl.unwrap_or(false);
    let target = if is_wsl { Target::Wsl } else { Target::Windows };
    let scan_path = if is_wsl { path.clone() } else { strip_win_prefix(&path) };

    let scan_dir = Path::new(&scan_path);
    let sub_services = scan_subprojects(scan_dir);

    let services: Vec<devlaunch_core::model::Service> = if sub_services.is_empty() {
        let pt = devlaunch_core::config::detect_project_type(scan_dir);
        let cmd = default_command_for_dir(pt, scan_dir);
        vec![devlaunch_core::model::Service {
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
            devlaunch_core::model::Service {
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

    let project = devlaunch_core::model::Project {
        name: name.clone(),
        description: String::new(),
        path: path.clone(),
        project_type: Some(devlaunch_core::model::ProjectType::Unknown),
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
pub async fn remove_project_cmd(name: String, state: State<'_, AppState>) -> Result<bool, String> {
    let mut config = load_global_config().map_err(|e| e.to_string())?;

    // Stop services if running
    if let Some(project) = devlaunch_core::global_config::find_project(&config, &name).cloned() {
        let mgr = state.get_manager(&project).await;
        let _ = mgr.stop_all().await;
    }

    // Remove from managers cache
    {
        let mut managers = state.managers.lock().await;
        managers.remove(&name);
    }

    devlaunch_core::global_config::remove_project(&mut config, &name);
    save_global_config(&config).map_err(|e| e.to_string())?;
    Ok(true)
}
