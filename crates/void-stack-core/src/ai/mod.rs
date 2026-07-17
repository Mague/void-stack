//! AI-powered contextual suggestions via configurable providers (Ollama, etc.).
//!
//! Analyzes code analysis results and generates actionable refactoring suggestions
//! using local LLMs.

mod config;
pub mod ollama;
pub mod prompt;
mod suggestions;

pub use config::{AiConfig, AiProvider, load_ai_config, save_ai_config};
pub(crate) use suggestions::parse_suggestions;
pub use suggestions::{Suggestion, SuggestionPriority, SuggestionResult, extract_file_paths};

use crate::analyzer::AnalysisResult;
use crate::error::Result;

// ── Public API ───────────────────────────────────────────────

/// Generate AI-powered suggestions from analysis results.
///
/// If the AI provider is not available, returns an error with the analysis context
/// that can be used directly by an AI assistant.
///
/// When a semantic index exists for the project, enriches the prompt with actual
/// code snippets from complexity hotspots and god classes.
pub async fn suggest(
    config: &AiConfig,
    analysis: &AnalysisResult,
    project_name: &str,
) -> Result<SuggestionResult> {
    let context = prompt::build_prompt(analysis, project_name);

    match config.provider {
        AiProvider::Ollama => ollama::generate(&config.base_url, &config.model, &context).await,
    }
}

/// Generate AI-powered suggestions, optionally enriched with code from the vector index.
pub async fn suggest_with_project(
    config: &AiConfig,
    analysis: &AnalysisResult,
    project: &crate::model::Project,
) -> Result<SuggestionResult> {
    let code_contexts = gather_code_contexts(analysis, project);
    let context = prompt::build_prompt_with_context(analysis, &project.name, &code_contexts);

    match config.provider {
        AiProvider::Ollama => ollama::generate(&config.base_url, &config.model, &context).await,
    }
}

/// Build the analysis context string without calling the AI provider.
/// Useful as a fallback when the provider is unavailable.
pub fn build_context(analysis: &AnalysisResult, project_name: &str) -> String {
    prompt::build_prompt(analysis, project_name)
}

/// Build context enriched with code from the semantic index.
pub fn build_context_with_project(
    analysis: &AnalysisResult,
    project: &crate::model::Project,
) -> String {
    let code_contexts = gather_code_contexts(analysis, project);
    prompt::build_prompt_with_context(analysis, &project.name, &code_contexts)
}

/// Gather code contexts from the semantic index for hotspots in the analysis.
#[cfg(not(feature = "vector"))]
fn gather_code_contexts(
    _analysis: &AnalysisResult,
    _project: &crate::model::Project,
) -> Vec<prompt::CodeContext> {
    Vec::new()
}

/// Gather code contexts from the semantic index for hotspots in the analysis.
#[cfg(feature = "vector")]
fn gather_code_contexts(
    analysis: &AnalysisResult,
    project: &crate::model::Project,
) -> Vec<prompt::CodeContext> {
    if !crate::vector_index::index_exists(project) {
        return Vec::new();
    }

    let mut contexts = Vec::new();
    let mut queries: Vec<(String, String)> = Vec::new();

    // Collect high-complexity functions (CC >= 10)
    if let Some(ref complexity) = analysis.complexity {
        for (file, fc) in complexity {
            for func in fc.complex_functions(10) {
                queries.push((
                    format!("{} {}", func.name, file),
                    format!("{}:{}() — CC {}", file, func.name, func.complexity),
                ));
            }
        }
    }

    // Collect god classes from anti-patterns
    for ap in &analysis.architecture.anti_patterns {
        if ap.kind == crate::analyzer::patterns::antipatterns::AntiPatternKind::GodClass {
            for module in &ap.affected_modules {
                queries.push((module.clone(), format!("God Class: {}", module)));
            }
        }
    }

    // Limit to top 5 hotspots to keep prompt manageable
    queries.truncate(5);

    for (query, label) in &queries {
        if let Ok(results) = crate::vector_index::semantic_search(project, query, 3) {
            let chunks: Vec<String> = results.into_iter().map(|r| r.chunk).collect();
            if !chunks.is_empty() {
                contexts.push(prompt::CodeContext {
                    label: label.clone(),
                    chunks,
                });
            }
        }
    }

    contexts
}
