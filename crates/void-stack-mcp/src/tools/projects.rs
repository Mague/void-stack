use rmcp::ErrorData as McpError;
use rmcp::model::*;
use tracing::info;

use void_stack_core::global_config::{
    GlobalConfig, default_command_for_dir, find_project, remove_project, save_global_config,
    scan_subprojects,
};
use void_stack_core::model::{DockerConfig, Project, Service, Target};
use void_stack_core::runner::local::{is_wsl_unc_path, strip_win_prefix};

use super::to_json_pretty;
use crate::server::{AddServiceRequest, ProjectInfo, ServiceInfo, VoidStackMcp};

/// Logic for list_projects tool.
pub fn list_projects(config: &GlobalConfig) -> Result<CallToolResult, McpError> {
    let projects: Vec<ProjectInfo> = config
        .projects
        .iter()
        .map(|p| ProjectInfo {
            name: p.name.clone(),
            path: p.path.clone(),
            project_type: p
                .project_type
                .map(|t| format!("{:?}", t))
                .unwrap_or_else(|| "Unknown".into()),
            services: p
                .services
                .iter()
                .map(|s| ServiceInfo {
                    name: s.name.clone(),
                    command: s.command.clone(),
                    target: s.target.to_string(),
                    working_dir: s.working_dir.clone(),
                    enabled: s.enabled,
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
                })
                .collect(),
        })
        .collect();

    let json = to_json_pretty(&projects)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Logic for add_project tool.
pub fn add_project(
    name: &str,
    path: &str,
    wsl: bool,
    distro: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let is_wsl = wsl || is_wsl_unc_path(path);
    let target = if is_wsl { Target::Wsl } else { Target::Windows };

    // For WSL: convert Linux path to UNC if needed
    let project_path = if is_wsl && !is_wsl_unc_path(path) {
        let distro = distro.ok_or_else(|| {
            McpError::invalid_params(
                "WSL projects require a 'distro' parameter (e.g., \"Ubuntu\")".to_string(),
                None,
            )
        })?;
        format!(r"\\wsl.localhost\{}{}", distro, path.replace('/', r"\"))
    } else {
        path.to_string()
    };

    let fs_path = std::path::Path::new(&project_path);
    if !fs_path.exists() {
        return Err(McpError::invalid_params(
            format!("Path '{}' does not exist", project_path),
            None,
        ));
    }

    let mut config = VoidStackMcp::load_config()?;

    // Check if already registered
    if find_project(&config, name).is_some() {
        return Err(McpError::invalid_params(
            format!("Project '{}' already exists", name),
            None,
        ));
    }

    // Scan for sub-projects
    let detected = scan_subprojects(fs_path);
    let services: Vec<Service> = detected
        .iter()
        .map(|(svc_name, sub_path, pt)| Service {
            name: svc_name.clone(),
            command: default_command_for_dir(*pt, sub_path),
            target,
            working_dir: Some(sub_path.to_string_lossy().to_string()),
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
            docker: None,
        })
        .collect();

    let project_type = detected.first().map(|(_, _, pt)| *pt);

    let project = Project {
        name: name.to_string(),
        description: String::new(),
        path: project_path.clone(),
        project_type,
        tags: vec![],
        services: services.clone(),
        hooks: None,
    };

    config.projects.push(project);
    save_global_config(&config)
        .map_err(|e| McpError::internal_error(format!("Failed to save config: {}", e), None))?;

    info!(project = %name, services = services.len(), "MCP: Project registered");

    let service_list: Vec<String> = services
        .iter()
        .map(|s| format!("  - {} ({})", s.name, s.command))
        .collect();

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Project '{}' registered with {} services:\n{}",
        name,
        services.len(),
        service_list.join("\n"),
    ))]))
}

/// Logic for remove_project tool.
pub async fn remove_project_tool(
    mcp: &VoidStackMcp,
    project_name: &str,
) -> Result<CallToolResult, McpError> {
    let mut config = VoidStackMcp::load_config()?;

    // Stop services if running
    if let Some(project) = find_project(&config, project_name).cloned() {
        let mgr = mcp.get_manager(&project).await;
        let _ = mgr.stop_all().await;

        // Remove from active managers
        let mut managers = mcp.managers.lock().await;
        managers.remove(&project.name);
    }

    if remove_project(&mut config, project_name) {
        save_global_config(&config)
            .map_err(|e| McpError::internal_error(format!("Failed to save config: {}", e), None))?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Project '{}' removed.",
            project_name,
        ))]))
    } else {
        Err(McpError::invalid_params(
            format!("Project '{}' not found", project_name),
            None,
        ))
    }
}

/// Logic for scan_directory tool.
pub fn scan_directory(path_str: &str) -> Result<CallToolResult, McpError> {
    let clean = strip_win_prefix(path_str);
    let path = std::path::Path::new(&clean);
    if !path.exists() {
        return Err(McpError::invalid_params(
            format!("Path '{}' does not exist", path_str),
            None,
        ));
    }

    let detected = scan_subprojects(path);

    if detected.is_empty() {
        let pt = void_stack_core::config::detect_project_type(path);
        let cmd = default_command_for_dir(pt, path);
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No sub-projects found. Root detected as {:?}.\n\nSuggested service:\n  - name: {}\n  - command: {}\n  - type: {:?}",
            pt,
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "main".into()),
            cmd,
            pt,
        ))]));
    }

    let mut lines = vec![format!(
        "Found {} service(s) at '{}':\n",
        detected.len(),
        clean
    )];
    for (name, sub_path, pt) in &detected {
        let cmd = default_command_for_dir(*pt, sub_path);
        let rel = sub_path.strip_prefix(path).unwrap_or(sub_path);
        lines.push(format!(
            "  - {} ({:?})\n    path: {}\n    command: {}",
            name,
            pt,
            rel.display(),
            cmd,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(
        lines.join("\n"),
    )]))
}

/// Logic for add_service tool.
pub fn add_service(params: &AddServiceRequest) -> Result<CallToolResult, McpError> {
    let mut config = VoidStackMcp::load_config()?;

    let project = config
        .projects
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(&params.project))
        .ok_or_else(|| {
            McpError::invalid_params(format!("Project '{}' not found", params.project), None)
        })?;

    // Check for duplicate service name
    if project
        .services
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&params.name))
    {
        return Err(McpError::invalid_params(
            format!(
                "Service '{}' already exists in project '{}'",
                params.name, project.name
            ),
            None,
        ));
    }

    let target = match params.target.as_deref() {
        Some(t) if t.eq_ignore_ascii_case("wsl") => Target::Wsl,
        Some(t) if t.eq_ignore_ascii_case("docker") => Target::Docker,
        _ => Target::Windows,
    };

    // Build Docker config if target is docker and any docker options provided
    let docker = if target == Target::Docker {
        let ports = params.docker_ports.clone().unwrap_or_default();
        let volumes = params.docker_volumes.clone().unwrap_or_default();
        let extra_args = params.docker_extra_args.clone().unwrap_or_default();
        if !ports.is_empty() || !volumes.is_empty() || !extra_args.is_empty() {
            Some(DockerConfig {
                ports,
                volumes,
                extra_args,
            })
        } else {
            None
        }
    } else {
        None
    };

    let service = Service {
        name: params.name.clone(),
        command: params.command.clone(),
        target,
        working_dir: Some(params.working_dir.clone()),
        enabled: true,
        env_vars: vec![],
        depends_on: vec![],
        docker,
    };

    let project_name = project.name.clone();
    project.services.push(service);

    save_global_config(&config)
        .map_err(|e| McpError::internal_error(format!("Failed to save config: {}", e), None))?;

    info!(
        project = %project_name,
        service = %params.name,
        "MCP: Service added"
    );

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Service '{}' added to project '{}' (target: {}, command: {})",
        params.name, project_name, target, params.command,
    ))]))
}
