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
use crate::server::VoidStackMcp;
use crate::types::{AddServiceRequest, ProjectInfo, ServiceInfo};

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

/// Logic for update_project tool: rename/move a project preserving all
/// derived data (indexes, structural graph, trust approval, git hook).
pub fn update_project_tool(
    project_name: &str,
    new_name: Option<&str>,
    new_path: Option<&str>,
) -> Result<CallToolResult, McpError> {
    if new_name.is_none() && new_path.is_none() {
        return Err(McpError::invalid_params(
            "nothing to change — pass new_name and/or new_path".to_string(),
            None,
        ));
    }

    // Snapshot the old project to re-key the in-memory watcher afterwards.
    let config = VoidStackMcp::load_config()?;
    let old_project = find_project(&config, project_name).cloned();

    let (updated, log) =
        void_stack_core::global_config::update_project(project_name, new_name, new_path)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

    // The watch registry is keyed by path: re-key a live watcher.
    if let Some(old) = &old_project
        && void_stack_core::vector_index::is_watching(old)
    {
        void_stack_core::vector_index::unwatch_project(old);
        if let Err(e) = void_stack_core::vector_index::watch_project(&updated) {
            info!("re-watch after update failed: {}", e);
        }
    }

    let mut lines = vec![format!(
        "Project updated: {} ({})",
        updated.name, updated.path
    )];
    lines.extend(log.iter().map(|l| format!("• {}", l)));
    if log.is_empty() {
        lines.push("• registry entry updated (no derived data needed migration)".to_string());
    }
    Ok(CallToolResult::success(vec![Content::text(
        lines.join("\n"),
    )]))
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

#[cfg(test)]
mod tests {
    use super::*;
    use void_stack_core::global_config::{load_global_config, save_global_config};
    use void_stack_core::model::{DockerConfig, ProjectType};

    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    // ── Pure formatting (no disk / no config) ───────────────────

    #[test]
    fn test_list_projects_empty() {
        let config = GlobalConfig::default();
        let out = text_of(&list_projects(&config).unwrap());
        assert_eq!(out.trim(), "[]");
    }

    #[test]
    fn test_list_projects_renders_services_and_docker() {
        let config = GlobalConfig {
            projects: vec![Project {
                name: "web".to_string(),
                description: String::new(),
                path: "C:/proj/web".to_string(),
                project_type: Some(ProjectType::Node),
                tags: vec![],
                services: vec![
                    Service {
                        name: "frontend".to_string(),
                        command: "npm run dev".to_string(),
                        target: Target::Windows,
                        working_dir: Some("C:/proj/web".to_string()),
                        enabled: true,
                        env_vars: vec![],
                        depends_on: vec![],
                        docker: None,
                    },
                    Service {
                        name: "db".to_string(),
                        command: "postgres:16".to_string(),
                        target: Target::Docker,
                        working_dir: None,
                        enabled: true,
                        env_vars: vec![],
                        depends_on: vec![],
                        docker: Some(DockerConfig {
                            ports: vec!["5432:5432".to_string()],
                            volumes: vec!["./data:/var/lib/postgresql/data".to_string()],
                            extra_args: vec![],
                        }),
                    },
                ],
                hooks: None,
            }],
            ..Default::default()
        };

        let out = text_of(&list_projects(&config).unwrap());
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        let p = &json[0];
        assert_eq!(p["name"], "web");
        assert_eq!(p["project_type"], "Node");
        let svcs = p["services"].as_array().unwrap();
        assert_eq!(svcs.len(), 2);
        assert_eq!(svcs[0]["target"], "windows");
        assert_eq!(svcs[1]["target"], "docker");
        assert_eq!(svcs[1]["docker_ports"][0], "5432:5432");
    }

    // ── scan_directory (tempdir, no config) ─────────────────────

    #[test]
    fn test_scan_directory_nonexistent() {
        let err = scan_directory("Z:\\no\\such\\dir\\ever").unwrap_err();
        assert!(err.message.contains("does not exist"));
    }

    #[test]
    fn test_scan_directory_no_subprojects() {
        let tmp = tempfile::tempdir().unwrap();
        let out = text_of(&scan_directory(&tmp.path().to_string_lossy()).unwrap());
        assert!(out.contains("No sub-projects found"), "got: {out}");
    }

    #[test]
    fn test_scan_directory_detects_node_subproject() {
        let tmp = tempfile::tempdir().unwrap();
        let front = tmp.path().join("frontend");
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(front.join("package.json"), "{\"name\":\"f\"}").unwrap();

        let out = text_of(&scan_directory(&tmp.path().to_string_lossy()).unwrap());
        assert!(out.contains("frontend"), "got: {out}");
    }

    // ── add_project guard branches (error before config load) ───

    #[test]
    fn test_add_project_path_not_found() {
        let err = add_project("ghost", "Z:\\no\\such\\path", false, None).unwrap_err();
        assert!(err.message.contains("does not exist"));
    }

    #[test]
    fn test_add_project_wsl_requires_distro() {
        let err = add_project("wsl-proj", "/home/u/app", true, None).unwrap_err();
        assert!(err.message.contains("distro"), "got: {}", err.message);
    }

    // ── Config-mutating lifecycle (serialized) ──────────────────

    #[tokio::test]
    async fn test_add_project_service_and_remove_lifecycle() {
        crate::tools::isolate_test_data_dir();
        let _guard = crate::tools::config_test_guard().await;
        // Start from a known-empty registry inside the isolated data dir.
        save_global_config(&GlobalConfig::default()).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let name = format!("proj-lifecycle-{}", std::process::id());

        // add_project on an empty dir registers with zero detected services.
        let out = text_of(&add_project(&name, &tmp.path().to_string_lossy(), false, None).unwrap());
        assert!(out.contains("registered"), "got: {out}");

        // add_service appends a service.
        let params = AddServiceRequest {
            project: name.clone(),
            name: "api".to_string(),
            command: "echo hi".to_string(),
            working_dir: tmp.path().to_string_lossy().to_string(),
            target: None,
            docker_ports: None,
            docker_volumes: None,
            docker_extra_args: None,
        };
        let out = text_of(&add_service(&params).unwrap());
        assert!(out.contains("added to project"), "got: {out}");

        // Duplicate service name is rejected.
        let err = add_service(&params).unwrap_err();
        assert!(
            err.message.contains("already exists"),
            "got: {}",
            err.message
        );

        // add_service to an unknown project is rejected.
        let bad = AddServiceRequest {
            project: "no-such-project".to_string(),
            name: "api".to_string(),
            command: "echo".to_string(),
            working_dir: ".".to_string(),
            target: None,
            docker_ports: None,
            docker_volumes: None,
            docker_extra_args: None,
        };
        let err = add_service(&bad).unwrap_err();
        assert!(err.message.contains("not found"), "got: {}", err.message);

        // The service persisted to the isolated config.
        let config = load_global_config().unwrap();
        let p = find_project(&config, &name).expect("project persisted");
        assert!(p.services.iter().any(|s| s.name == "api"));

        // remove_project_tool deletes it; removing again is a not-found error.
        let mcp = VoidStackMcp::new();
        let out = text_of(&remove_project_tool(&mcp, &name).await.unwrap());
        assert!(out.contains("removed"), "got: {out}");
        let err = remove_project_tool(&mcp, &name).await.unwrap_err();
        assert!(err.message.contains("not found"), "got: {}", err.message);
    }
}
