use rmcp::ErrorData as McpError;
use rmcp::model::*;
use tracing::info;

use void_stack_core::global_config::{
    GlobalConfig, ServiceUpdate, TagEdit, apply_tag_edit, default_command_for_dir, edit_service,
    find_project, remove_project, remove_service, save_global_config, scan_subprojects,
    update_project_tags,
};
use void_stack_core::model::{DockerConfig, Project, Service, Target};
use void_stack_core::runner::local::{is_wsl_unc_path, strip_win_prefix};

use super::to_json_pretty;
use crate::server::VoidStackMcp;
use crate::types::{
    AddServiceRequest, ProjectInfo, RemoveServiceRequest, ServiceInfo, UpdateProjectRequest,
    UpdateServiceRequest,
};

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
            tags: p.tags.clone(),
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
    tags: &[String],
) -> Result<CallToolResult, McpError> {
    // Validate tags first so a typo never leaves a half-registered project.
    let tags = apply_tag_edit(
        &[],
        &TagEdit {
            add: tags.to_vec(),
            ..Default::default()
        },
    )
    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
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
        tags: tags.clone(),
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

    let tag_line = if tags.is_empty() {
        String::new()
    } else {
        format!("\nTags: {}", tags.join(", "))
    };
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Project '{}' registered with {} services:\n{}{}",
        name,
        services.len(),
        service_list.join("\n"),
        tag_line,
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
/// derived data (indexes, structural graph, trust approval, git hook),
/// and/or edit its tags.
pub fn update_project_tool(req: &UpdateProjectRequest) -> Result<CallToolResult, McpError> {
    let new_name = req.new_name.as_deref();
    let new_path = req.new_path.as_deref();
    let tag_edit = TagEdit {
        set: req.tags.clone(),
        add: req.add_tags.clone(),
        remove: req.remove_tags.clone(),
    };
    if new_name.is_none() && new_path.is_none() && tag_edit.is_empty() {
        return Err(McpError::invalid_params(
            "nothing to change — pass new_name, new_path and/or tags".to_string(),
            None,
        ));
    }
    // Validate the tags up front: a bad tag must not leave a half-applied rename.
    let config = VoidStackMcp::load_config()?;
    let old_project = find_project(&config, &req.project).cloned();
    if let Some(old) = &old_project {
        apply_tag_edit(&old.tags, &tag_edit)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
    }

    let mut lines = Vec::new();
    let mut current_name = req.project.clone();
    if new_name.is_some() || new_path.is_some() {
        let (updated, log) =
            void_stack_core::global_config::update_project(&req.project, new_name, new_path)
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

        lines.push(format!(
            "Project updated: {} ({})",
            updated.name, updated.path
        ));
        lines.extend(log.iter().map(|l| format!("• {}", l)));
        if log.is_empty() {
            lines.push("• registry entry updated (no derived data needed migration)".to_string());
        }
        current_name = updated.name;
    }

    if !tag_edit.is_empty() {
        let tagged = update_project_tags(&current_name, &tag_edit)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        lines.push(format!(
            "Tags for {}: {}",
            tagged.name,
            if tagged.tags.is_empty() {
                "(none)".to_string()
            } else {
                tagged.tags.join(", ")
            }
        ));
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

/// Logic for update_service tool.
pub fn update_service_tool(params: &UpdateServiceRequest) -> Result<CallToolResult, McpError> {
    let invalid = |e: String| McpError::invalid_params(e, None);
    let update = ServiceUpdate {
        name: params.new_name.clone(),
        command: params.command.clone(),
        working_dir: params.working_dir.clone(),
        enabled: params.enabled,
        ..Default::default()
    }
    .target_named(params.target.as_deref())
    .map_err(invalid)?
    .docker_lists(
        params.docker_ports.clone(),
        params.docker_volumes.clone(),
        params.docker_extra_args.clone(),
    );
    let saved = edit_service(&params.project, &params.service, update).map_err(invalid)?;
    info!(project = %params.project, service = %saved.name, "MCP: Service updated");

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Service '{}' updated in project '{}': command `{}`, target {}, working dir {}, {}. \
         Restart it (stop_service + start_service) if it is running.",
        saved.name,
        params.project,
        saved.command,
        saved.target,
        saved.working_dir.as_deref().unwrap_or("(project root)"),
        if saved.enabled { "enabled" } else { "disabled" },
    ))]))
}

/// Logic for remove_service tool.
pub fn remove_service_tool(params: &RemoveServiceRequest) -> Result<CallToolResult, McpError> {
    let mut config = VoidStackMcp::load_config()?;
    if !remove_service(&mut config, &params.project, &params.service) {
        return Err(McpError::invalid_params(
            format!(
                "Service '{}' not found in project '{}'",
                params.service, params.project
            ),
            None,
        ));
    }
    save_global_config(&config)
        .map_err(|e| McpError::internal_error(format!("Failed to save config: {}", e), None))?;
    info!(project = %params.project, service = %params.service, "MCP: Service removed");
    Ok(CallToolResult::success(vec![Content::text(format!(
        "Service '{}' removed from project '{}'",
        params.service, params.project
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
        let err = add_project("ghost", "Z:\\no\\such\\path", false, None, &[]).unwrap_err();
        assert!(err.message.contains("does not exist"));
    }

    #[test]
    fn test_add_project_wsl_requires_distro() {
        let err = add_project("wsl-proj", "/home/u/app", true, None, &[]).unwrap_err();
        assert!(err.message.contains("distro"), "got: {}", err.message);
    }

    #[tokio::test]
    async fn test_project_tags_add_list_update() {
        crate::tools::isolate_test_data_dir();
        let _guard = crate::tools::config_test_guard().await;
        save_global_config(&GlobalConfig::default()).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let name = format!("proj-tags-{}", std::process::id());
        let path = tmp.path().to_string_lossy().to_string();

        // A bad tag is rejected before anything is registered.
        let err = add_project(&name, &path, false, None, &["no/slash".into()]).unwrap_err();
        assert!(err.message.contains("invalid character"), "{}", err.message);
        assert!(find_project(&load_global_config().unwrap(), &name).is_none());

        let out = text_of(&add_project(&name, &path, false, None, &["Tercero".into()]).unwrap());
        assert!(out.contains("Tags: third-party"), "got: {out}");
        let listed = text_of(&list_projects(&load_global_config().unwrap()).unwrap());
        assert!(listed.contains("\"third-party\""), "got: {listed}");

        let req = |add: &[&str], remove: &[&str]| UpdateProjectRequest {
            project: name.clone(),
            new_name: None,
            new_path: None,
            tags: None,
            add_tags: add.iter().map(|t| t.to_string()).collect(),
            remove_tags: remove.iter().map(|t| t.to_string()).collect(),
        };
        let out = text_of(&update_project_tool(&req(&["propio", "trading"], &[])).unwrap());
        assert!(out.contains("Tags for"), "got: {out}");
        let tags = find_project(&load_global_config().unwrap(), &name)
            .unwrap()
            .tags
            .clone();
        assert_eq!(tags, vec!["own".to_string(), "trading".to_string()]);

        text_of(&update_project_tool(&req(&[], &["trading"])).unwrap());
        let tags = find_project(&load_global_config().unwrap(), &name)
            .unwrap()
            .tags
            .clone();
        assert_eq!(tags, vec!["own".to_string()]);

        let err = update_project_tool(&req(&[], &[])).unwrap_err();
        assert!(err.message.contains("nothing to change"), "{}", err.message);
        assert!(update_project_tool(&req(&["a b/c"], &[])).is_err());
        assert!(remove_project_from_config(&name));
    }

    /// Test helper: drop a project from the isolated registry.
    fn remove_project_from_config(name: &str) -> bool {
        let mut config = load_global_config().unwrap();
        let removed = remove_project(&mut config, name);
        save_global_config(&config).unwrap();
        removed
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
        let out =
            text_of(&add_project(&name, &tmp.path().to_string_lossy(), false, None, &[]).unwrap());
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

        // update_service fixes the command and target in place.
        let update = |service: &str| UpdateServiceRequest {
            project: name.clone(),
            service: service.to_string(),
            new_name: Some("backend".to_string()),
            command: Some("pnpm dev".to_string()),
            working_dir: None,
            target: Some("wsl".to_string()),
            enabled: None,
            docker_ports: None,
            docker_volumes: None,
            docker_extra_args: None,
        };
        let out = text_of(&update_service_tool(&update("API")).unwrap());
        assert!(out.contains("'backend' updated"), "got: {out}");
        let config = load_global_config().unwrap();
        let svc = &find_project(&config, &name).unwrap().services[0];
        assert_eq!(
            (svc.name.as_str(), svc.command.as_str(), svc.target),
            ("backend", "pnpm dev", Target::Wsl)
        );
        let err = update_service_tool(&update("api")).unwrap_err();
        assert!(err.message.contains("not found"), "got: {}", err.message);
        let mut bad_target = update("backend");
        bad_target.target = Some("mars".to_string());
        let err = update_service_tool(&bad_target).unwrap_err();
        assert!(
            err.message.contains("Unknown target"),
            "got: {}",
            err.message
        );

        // remove_service drops it; a second removal is a not-found error.
        let rm = RemoveServiceRequest {
            project: name.clone(),
            service: "backend".to_string(),
        };
        assert!(text_of(&remove_service_tool(&rm).unwrap()).contains("removed"));
        assert!(remove_service_tool(&rm).is_err());

        // remove_project_tool deletes it; removing again is a not-found error.
        let mcp = VoidStackMcp::new();
        let out = text_of(&remove_project_tool(&mcp, &name).await.unwrap());
        assert!(out.contains("removed"), "got: {out}");
        let err = remove_project_tool(&mcp, &name).await.unwrap_err();
        assert!(err.message.contains("not found"), "got: {}", err.message);
    }
}
