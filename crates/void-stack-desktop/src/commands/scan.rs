use serde::Serialize;

use void_stack_core::config::detect_project_type;
use void_stack_core::global_config::{
    load_global_config, save_global_config, scan_subprojects, default_command_for_dir,
};
use void_stack_core::model::Target;
use void_stack_core::runner::local::strip_win_prefix;


#[derive(Serialize)]
pub struct ScanResultDto {
    pub services: Vec<ScannedServiceDto>,
    pub project_type: String,
}

#[derive(Serialize)]
pub struct ScannedServiceDto {
    pub name: String,
    pub command: String,
    pub working_dir: String,
    pub detected_type: String,
}

/// Scan a directory without registering it — preview what would be detected.
#[tauri::command]
pub fn scan_directory(path: String) -> Result<ScanResultDto, String> {
    let clean = strip_win_prefix(&path);
    let scan_path = std::path::Path::new(&clean);

    if !scan_path.exists() {
        return Err(format!("La ruta '{}' no existe", path));
    }

    let subs = scan_subprojects(scan_path);
    let project_type = detect_project_type(scan_path);

    let services: Vec<ScannedServiceDto> = if subs.is_empty() {
        let cmd = default_command_for_dir(project_type, scan_path);
        vec![ScannedServiceDto {
            name: scan_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "default".to_string()),
            command: cmd,
            working_dir: path.clone(),
            detected_type: format!("{:?}", project_type),
        }]
    } else {
        subs.into_iter().map(|(name, svc_path, svc_type)| {
            let cmd = default_command_for_dir(svc_type, &svc_path);
            ScannedServiceDto {
                name,
                command: cmd,
                working_dir: svc_path.to_string_lossy().to_string(),
                detected_type: format!("{:?}", svc_type),
            }
        }).collect()
    };

    Ok(ScanResultDto {
        services,
        project_type: format!("{:?}", project_type),
    })
}

/// Manually add a service to an existing project.
#[tauri::command]
pub fn add_service_cmd(
    project: String,
    name: String,
    command: String,
    working_dir: String,
    target: Option<String>,
) -> Result<bool, String> {
    let mut config = load_global_config().map_err(|e| e.to_string())?;
    let proj = config.projects.iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(&project))
        .ok_or_else(|| format!("Proyecto '{}' no encontrado", project))?;

    if proj.services.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
        return Err(format!("El servicio '{}' ya existe en '{}'", name, project));
    }

    let tgt = match target.as_deref() {
        Some("wsl") | Some("WSL") => Target::Wsl,
        Some("docker") | Some("Docker") => Target::Docker,
        _ => Target::Windows,
    };

    proj.services.push(void_stack_core::model::Service {
        name,
        command,
        target: tgt,
        working_dir: Some(working_dir),
        enabled: true,
        env_vars: Vec::new(),
        depends_on: Vec::new(),
    });

    save_global_config(&config).map_err(|e| e.to_string())?;
    Ok(true)
}
