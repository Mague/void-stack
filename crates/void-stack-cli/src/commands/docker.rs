use std::path::Path;

use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};

pub fn cmd_docker(
    project_name: &str,
    gen_dockerfile: bool,
    gen_compose: bool,
    save: bool,
) -> Result<()> {
    use void_stack_core::config;
    use void_stack_core::docker;
    use void_stack_core::runner::local::strip_win_prefix;

    let config = load_global_config()?;
    let proj = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Proyecto '{}' no encontrado", project_name))?;
    let clean_path = strip_win_prefix(&proj.path);
    let project_path = Path::new(&clean_path);

    // 1. Analyze existing Docker artifacts
    let analysis = docker::analyze_docker(project_path);

    println!("\n  Docker Analysis: {}", proj.name);
    println!("  {}", "─".repeat(40));

    if analysis.has_dockerfile {
        println!("  ✅ Dockerfile encontrado");
        if let Some(ref df) = analysis.dockerfile {
            for (i, stage) in df.stages.iter().enumerate() {
                let name = stage.name.as_deref().unwrap_or("(unnamed)");
                println!("     Stage {}: {} ({})", i, stage.base_image, name);
            }
            if !df.exposed_ports.is_empty() {
                println!("     Ports: {:?}", df.exposed_ports);
            }
            if let Some(ref cmd) = df.cmd {
                println!("     CMD: {}", cmd);
            }
        }
    } else {
        println!("  ⚠ No Dockerfile");
    }

    if analysis.has_compose {
        println!("  ✅ docker-compose encontrado");
        if let Some(ref compose) = analysis.compose {
            for svc in &compose.services {
                let ports: Vec<String> = svc
                    .ports
                    .iter()
                    .map(|p| format!("{}:{}", p.host, p.container))
                    .collect();
                let ports_str = if ports.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", ports.join(", "))
                };
                let img = svc.image.as_deref().unwrap_or("build");
                println!("     {} ({}) → {}{}", svc.name, svc.kind, img, ports_str);
            }
        }
    } else {
        println!("  ⚠ No docker-compose");
    }

    // Terraform
    if !analysis.terraform.is_empty() {
        println!(
            "\n  ── Terraform ({} recursos) ──",
            analysis.terraform.len()
        );
        for res in &analysis.terraform {
            let details = if res.details.is_empty() {
                String::new()
            } else {
                format!(" ({})", res.details.join(", "))
            };
            println!(
                "     [{}] {} \"{}\" → {}{}",
                res.provider, res.resource_type, res.name, res.kind, details
            );
        }
    }

    // Kubernetes
    if !analysis.kubernetes.is_empty() {
        println!(
            "\n  ── Kubernetes ({} recursos) ──",
            analysis.kubernetes.len()
        );
        for res in &analysis.kubernetes {
            let ns = res.namespace.as_deref().unwrap_or("default");
            let images = if res.images.is_empty() {
                String::new()
            } else {
                format!(" images=[{}]", res.images.join(", "))
            };
            let ports = if res.ports.is_empty() {
                String::new()
            } else {
                format!(" ports={:?}", res.ports)
            };
            let replicas = res.replicas.map(|r| format!(" x{}", r)).unwrap_or_default();
            println!(
                "     {}: {} (ns={}){}{}{}",
                res.kind, res.name, ns, replicas, images, ports
            );
        }
    }

    // Helm
    if let Some(ref chart) = analysis.helm {
        println!("\n  ── Helm: {} v{} ──", chart.name, chart.version);
        if !chart.dependencies.is_empty() {
            for dep in &chart.dependencies {
                println!(
                    "     dep: {} ({}) → {}",
                    dep.name, dep.version, dep.repository
                );
            }
        }
    }

    // 2. Generate Dockerfile
    if gen_dockerfile && !analysis.has_dockerfile {
        let project_type = config::detect_project_type(project_path);
        if let Some(content) = docker::generate_dockerfile::generate(project_path, project_type) {
            println!("\n  ── Dockerfile generado ──\n");
            for line in content.lines() {
                println!("  {}", line);
            }
            if save {
                let out = project_path.join("Dockerfile");
                std::fs::write(&out, &content)?;
                println!("\n  ✅ Guardado en {}", out.display());
            }
        } else {
            println!(
                "\n  ⚠ No se pudo generar Dockerfile para tipo {:?}",
                config::detect_project_type(project_path)
            );
        }
    } else if gen_dockerfile && analysis.has_dockerfile {
        println!("\n  ℹ Dockerfile ya existe, no se sobreescribe");
    }

    // 3. Generate docker-compose.yml
    if gen_compose {
        let content = docker::generate_compose::generate(proj, project_path);
        println!("\n  ── docker-compose.yml generado ──\n");
        for line in content.lines() {
            println!("  {}", line);
        }
        if save {
            let out = project_path.join("docker-compose.yml");
            std::fs::write(&out, &content)?;
            println!("\n  ✅ Guardado en {}", out.display());
        }
    }

    if !gen_dockerfile && !gen_compose {
        println!("\n  Usa --generate-dockerfile y/o --generate-compose para generar archivos");
    }

    println!();
    Ok(())
}
