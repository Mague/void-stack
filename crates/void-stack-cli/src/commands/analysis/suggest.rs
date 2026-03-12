use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::runner::local::strip_win_prefix;

pub async fn cmd_suggest(project_name: &str, model_override: Option<&str>, service_filter: Option<&str>, raw: bool) -> Result<()> {
    use void_stack_core::ai;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    // Load AI config
    let mut ai_config = ai::load_ai_config().unwrap_or_default();
    if let Some(model) = model_override {
        ai_config.model = model.to_string();
    }

    println!("Analizando proyecto '{}'...\n", project.name);

    // Collect analysis results
    let services: Vec<_> = match service_filter {
        Some(svc_name) => {
            project.services.iter()
                .filter(|s| s.name.eq_ignore_ascii_case(svc_name))
                .collect()
        }
        None => project.services.iter().collect(),
    };

    let mut analysis = None;
    for svc in &services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let clean = strip_win_prefix(dir);
        let path = std::path::Path::new(&clean);
        if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
            analysis = Some(result);
            break; // Use first analyzable service
        }
    }

    let analysis = analysis
        .ok_or_else(|| anyhow::anyhow!("No se pudo analizar el proyecto (sin archivos fuente detectados)"))?;

    println!("Generando sugerencias con {} ({})...\n", ai_config.provider_name(), ai_config.model);

    match ai::suggest(&ai_config, &analysis, &project.name).await {
        Ok(result) => {
            if raw {
                println!("{}", result.raw_response);
            } else {
                println!("Modelo: {}\n", result.model_used);
                if result.suggestions.is_empty() {
                    println!("  No se generaron sugerencias estructuradas.");
                    println!("\nRespuesta completa:\n{}", result.raw_response);
                } else {
                    for (i, s) in result.suggestions.iter().enumerate() {
                        let priority_icon = match s.priority {
                            ai::SuggestionPriority::Critical => "!!",
                            ai::SuggestionPriority::High => "! ",
                            ai::SuggestionPriority::Medium => "- ",
                            ai::SuggestionPriority::Low => "  ",
                        };
                        println!("{}. {} [{}] {}", i + 1, priority_icon, s.category, s.title);
                        println!("   {}", s.description);
                        if !s.affected_files.is_empty() {
                            println!("   Archivos: {}", s.affected_files.join(", "));
                        }
                        println!();
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error de AI: {}\n", e);
            println!("Mostrando contexto de análisis que puedes usar con tu asistente AI:\n");
            let context = ai::build_context(&analysis, &project.name);
            println!("{}", context);
        }
    }

    Ok(())
}
