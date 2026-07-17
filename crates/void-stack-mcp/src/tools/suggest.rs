use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

/// Logic for suggest_refactoring tool.
pub async fn suggest_refactoring(
    project: &Project,
    service_name: Option<&str>,
    model: Option<&str>,
) -> Result<CallToolResult, McpError> {
    // Find first analyzable service directory
    let services: Vec<_> = match service_name {
        Some(svc_name) => {
            let needle = svc_name.to_ascii_lowercase();
            project
                .services
                .iter()
                .filter(|s| {
                    let name = s.name.to_ascii_lowercase();
                    name == needle
                        || name.ends_with(&format!("/{}", needle))
                        || name.ends_with(&format!("\\{}", needle))
                })
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
            break;
        }
    }

    let analysis = match analysis {
        Some(a) => a,
        None => {
            return Ok(CallToolResult::success(vec![Content::text(
                "No analyzable code found in the project (supported: Python, JavaScript/TypeScript, Go, Rust, Dart).".to_string(),
            )]));
        }
    };

    // Load AI config
    let mut ai_config = void_stack_core::ai::load_ai_config().unwrap_or_default();
    if let Some(model) = model {
        ai_config.model = model.to_string();
    }

    // Try to call Ollama; if unavailable, return the analysis context
    match void_stack_core::ai::suggest_with_project(&ai_config, &analysis, project).await {
        Ok(result) => {
            let mut output = format!("## Sugerencias de AI (modelo: {})\n\n", result.model_used);
            for (i, s) in result.suggestions.iter().enumerate() {
                output.push_str(&format!(
                    "### {}. [{}] {} ({})\n{}\n",
                    i + 1,
                    s.category,
                    s.title,
                    s.priority,
                    s.description,
                ));
                if !s.affected_files.is_empty() {
                    output.push_str(&format!("Archivos: {}\n", s.affected_files.join(", ")));
                }
                output.push('\n');
            }
            Ok(CallToolResult::success(vec![Content::text(output)]))
        }
        Err(_) => {
            // Fallback: return analysis context for the AI assistant to process directly
            let context = void_stack_core::ai::build_context_with_project(&analysis, project);
            let output = format!(
                "Ollama no está disponible. Aquí está el contexto de análisis para que generes sugerencias directamente:\n\n{}",
                context,
            );
            Ok(CallToolResult::success(vec![Content::text(output)]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(result: &CallToolResult) -> String {
        result.content[0]
            .as_text()
            .expect("tool result is text")
            .text
            .clone()
    }

    fn empty_project() -> Project {
        Project {
            name: "suggest-fixture".to_string(),
            description: String::new(),
            path: "C:/no/such/dir/void-suggest".to_string(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    /// With no services and an unanalyzable path, the handler returns the
    /// "no analyzable code" message before any AI/Ollama call — a pure,
    /// network-free branch.
    #[tokio::test]
    async fn test_suggest_refactoring_no_analyzable_code() {
        let out = suggest_refactoring(&empty_project(), None, None)
            .await
            .unwrap();
        assert!(text_of(&out).contains("No analyzable code found"));
    }

    /// A service filter that matches nothing narrows the service list to
    /// empty, which also short-circuits to the "no analyzable code" message.
    #[tokio::test]
    async fn test_suggest_refactoring_unknown_service_filter() {
        let out = suggest_refactoring(&empty_project(), Some("does-not-exist"), None)
            .await
            .unwrap();
        assert!(text_of(&out).contains("No analyzable code found"));
    }
}
