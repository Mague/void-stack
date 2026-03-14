use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

/// Logic for docker_analyze tool.
pub async fn docker_analyze(project: &Project) -> Result<CallToolResult, McpError> {
    let clean = strip_win_prefix(&project.path);
    let project_path = std::path::PathBuf::from(&clean);
    let proj_name = project.name.clone();

    let analysis =
        tokio::task::spawn_blocking(move || void_stack_core::docker::analyze_docker(&project_path))
            .await
            .map_err(|e| McpError::internal_error(format!("Analysis failed: {}", e), None))?;

    let mut lines = Vec::new();
    lines.push(format!("Docker Analysis: {}\n", proj_name));

    if analysis.has_dockerfile {
        lines.push("Dockerfile: found".to_string());
        if let Some(ref df) = analysis.dockerfile {
            for (i, stage) in df.stages.iter().enumerate() {
                let name = stage.name.as_deref().unwrap_or("(unnamed)");
                lines.push(format!("  Stage {}: {} ({})", i, stage.base_image, name));
            }
            if !df.exposed_ports.is_empty() {
                lines.push(format!("  Ports: {:?}", df.exposed_ports));
            }
        }
    } else {
        lines.push("Dockerfile: not found".to_string());
    }

    if analysis.has_compose {
        lines.push("docker-compose: found".to_string());
        if let Some(ref compose) = analysis.compose {
            for svc in &compose.services {
                let img = svc.image.as_deref().unwrap_or("build");
                let ports: Vec<String> = svc
                    .ports
                    .iter()
                    .map(|p| format!("{}:{}", p.host, p.container))
                    .collect();
                lines.push(format!(
                    "  {} ({}) → {} [{}]",
                    svc.name,
                    svc.kind,
                    img,
                    ports.join(", ")
                ));
            }
        }
    } else {
        lines.push("docker-compose: not found".to_string());
    }

    // Terraform
    if !analysis.terraform.is_empty() {
        lines.push(format!(
            "\nTerraform ({} resources):",
            analysis.terraform.len()
        ));
        for res in &analysis.terraform {
            let details = if res.details.is_empty() {
                String::new()
            } else {
                format!(" ({})", res.details.join(", "))
            };
            lines.push(format!(
                "  [{}] {} \"{}\" → {}{}",
                res.provider, res.resource_type, res.name, res.kind, details
            ));
        }
    }

    // Kubernetes
    if !analysis.kubernetes.is_empty() {
        lines.push(format!(
            "\nKubernetes ({} resources):",
            analysis.kubernetes.len()
        ));
        for res in &analysis.kubernetes {
            let ns = res.namespace.as_deref().unwrap_or("default");
            let images = if res.images.is_empty() {
                String::new()
            } else {
                format!(" images=[{}]", res.images.join(", "))
            };
            lines.push(format!(
                "  {}: {} (ns={}){}",
                res.kind, res.name, ns, images
            ));
        }
    }

    // Helm
    if let Some(ref chart) = analysis.helm {
        lines.push(format!("\nHelm: {} v{}", chart.name, chart.version));
        for dep in &chart.dependencies {
            lines.push(format!(
                "  dep: {} ({}) → {}",
                dep.name, dep.version, dep.repository
            ));
        }
    }

    Ok(CallToolResult::success(vec![Content::text(
        lines.join("\n"),
    )]))
}

/// Logic for docker_generate tool.
pub async fn docker_generate(
    project: &Project,
    generate_dockerfile: bool,
    generate_compose: bool,
    save: bool,
) -> Result<CallToolResult, McpError> {
    let clean = strip_win_prefix(&project.path);
    let project_path = std::path::PathBuf::from(&clean);
    let gen_df = generate_dockerfile;
    let gen_compose = generate_compose;

    let proj_clone = project.clone();
    let path_clone = project_path.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut dockerfile_content = None;
        let mut compose_content = None;
        let mut saved = Vec::new();

        if gen_df && !path_clone.join("Dockerfile").exists() {
            let pt = void_stack_core::config::detect_project_type(&path_clone);
            if let Some(content) =
                void_stack_core::docker::generate_dockerfile::generate(&path_clone, pt)
            {
                if save {
                    let out = path_clone.join("Dockerfile");
                    let _ = std::fs::write(&out, &content);
                    saved.push(out.to_string_lossy().to_string());
                }
                dockerfile_content = Some(content);
            }
        }

        if gen_compose {
            let content =
                void_stack_core::docker::generate_compose::generate(&proj_clone, &path_clone);
            if save {
                let out = path_clone.join("docker-compose.yml");
                let _ = std::fs::write(&out, &content);
                saved.push(out.to_string_lossy().to_string());
            }
            compose_content = Some(content);
        }

        (dockerfile_content, compose_content, saved)
    })
    .await
    .map_err(|e| McpError::internal_error(format!("Generation failed: {}", e), None))?;

    let mut output = Vec::new();

    if let Some(ref df) = result.0 {
        output.push(format!("── Generated Dockerfile ──\n\n{}", df));
    }
    if let Some(ref compose) = result.1 {
        output.push(format!("── Generated docker-compose.yml ──\n\n{}", compose));
    }
    if !result.2.is_empty() {
        output.push(format!("Saved to:\n{}", result.2.join("\n")));
    }

    if output.is_empty() {
        output.push(
            "No files generated (Dockerfile already exists or unsupported project type)."
                .to_string(),
        );
    }

    Ok(CallToolResult::success(vec![Content::text(
        output.join("\n\n"),
    )]))
}
