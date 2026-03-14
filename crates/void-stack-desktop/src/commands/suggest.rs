use serde::Serialize;

use void_stack_core::ai;
use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct SuggestionResultDto {
    pub suggestions: Vec<SuggestionDto>,
    pub model_used: String,
    pub raw_response: String,
    /// If Ollama is not available, this contains the analysis context
    pub fallback_context: Option<String>,
}

#[derive(Serialize)]
pub struct SuggestionDto {
    pub category: String,
    pub title: String,
    pub description: String,
    pub affected_files: Vec<String>,
    pub priority: String,
}

#[tauri::command]
pub async fn suggest_refactoring(
    project: String,
    model: Option<String>,
    service: Option<String>,
) -> Result<SuggestionResultDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    // Find first analyzable service
    let services: Vec<_> = match &service {
        Some(svc_name) => proj
            .services
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case(svc_name))
            .collect(),
        None => proj.services.iter().collect(),
    };

    let mut analysis = None;
    for svc in &services {
        let dir = svc.working_dir.as_deref().unwrap_or(&proj.path);
        let clean = strip_win_prefix(dir);
        let path = std::path::Path::new(&clean);
        if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
            analysis = Some(result);
            break;
        }
    }

    let analysis = analysis.ok_or_else(|| {
        "No se pudo analizar el proyecto (sin archivos fuente detectados)".to_string()
    })?;

    // Load AI config
    let mut ai_config = ai::load_ai_config().unwrap_or_default();
    if let Some(m) = model {
        ai_config.model = m;
    }

    // Try Ollama; fallback to context
    match ai::suggest(&ai_config, &analysis, &proj.name).await {
        Ok(result) => Ok(SuggestionResultDto {
            suggestions: result
                .suggestions
                .iter()
                .map(|s| SuggestionDto {
                    category: s.category.clone(),
                    title: s.title.clone(),
                    description: s.description.clone(),
                    affected_files: s.affected_files.clone(),
                    priority: s.priority.to_string(),
                })
                .collect(),
            model_used: result.model_used,
            raw_response: result.raw_response,
            fallback_context: None,
        }),
        Err(e) => {
            let context = ai::build_context(&analysis, &proj.name);
            Ok(SuggestionResultDto {
                suggestions: vec![],
                model_used: String::new(),
                raw_response: String::new(),
                fallback_context: Some(format!(
                    "Ollama no disponible ({}). Contexto de análisis:\n\n{}",
                    e, context
                )),
            })
        }
    }
}
