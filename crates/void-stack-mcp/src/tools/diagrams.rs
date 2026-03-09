use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

/// Logic for generate_diagram tool.
pub fn generate_diagram(project: &Project, format: Option<&str>) -> Result<CallToolResult, McpError> {
    let format = format.unwrap_or("drawio");
    let is_drawio =
        format.eq_ignore_ascii_case("drawio") || format.eq_ignore_ascii_case("draw.io");

    if is_drawio {
        let xml = void_stack_core::diagram::drawio::generate_all(project);
        let dir = strip_win_prefix(&project.path);
        let path = format!("{}/void-stack-diagrams.drawio", dir);
        std::fs::write(&path, &xml).map_err(|e| {
            McpError::internal_error(format!("Failed to write drawio file: {}", e), None)
        })?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Draw.io diagram saved to: {}\n\nOpen it with VS Code Draw.io extension or at diagrams.net",
            path
        ))]))
    } else {
        let diagrams = void_stack_core::diagram::generate_all(project);
        let mut content = format!(
            "# {} — Architecture\n\n## Service Architecture\n\n{}\n\n",
            project.name, diagrams.architecture
        );
        if let Some(api) = &diagrams.api_routes {
            content.push_str(&format!("## API Routes\n\n{}\n\n", api));
        }
        if let Some(db) = &diagrams.db_models {
            content.push_str(&format!("## Database Models\n\n{}\n\n", db));
        }
        if !diagrams.warnings.is_empty() {
            content.push_str("## Advertencias\n\n");
            for w in &diagrams.warnings {
                content.push_str(&format!("- {}\n", w));
            }
            content.push_str("\n");
        }
        Ok(CallToolResult::success(vec![Content::text(content)]))
    }
}
