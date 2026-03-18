use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::runner::local::strip_win_prefix;

pub fn cmd_diagram(
    project_name: &str,
    output: Option<&str>,
    format: &str,
    print_content: bool,
) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let is_drawio = format.eq_ignore_ascii_case("drawio") || format.eq_ignore_ascii_case("draw.io");

    if is_drawio {
        let content = void_stack_core::diagram::drawio::generate_all(project);
        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(&project.path);
                format!("{}/void-stack-diagrams.drawio", dir)
            }
        };
        std::fs::write(&path, &content)?;
        println!("Draw.io diagram saved to {}", path);
        if print_content {
            println!("\n{}", content);
        }
    } else {
        // Mermaid format
        let diagrams = void_stack_core::diagram::generate_all(project);
        let mut content = String::new();
        content.push_str(&format!("# {} — Architecture\n\n", project.name));
        content.push_str("## Service Architecture\n\n");
        content.push_str(&diagrams.architecture);
        content.push_str("\n\n");

        if let Some(api) = &diagrams.api_routes {
            content.push_str("## API Routes\n\n");
            content.push_str(api);
            content.push_str("\n\n");
        }

        if let Some(db) = &diagrams.db_models {
            content.push_str("## Database Models\n\n");
            content.push_str(db);
            content.push_str("\n\n");
        }

        if !diagrams.warnings.is_empty() {
            content.push_str("## Advertencias\n\n");
            for w in &diagrams.warnings {
                content.push_str(&format!("- {}\n", w));
            }
            content.push('\n');

            for w in &diagrams.warnings {
                println!("  Warning: {}", w);
            }
        }

        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(&project.path);
                format!("{}/void-stack-diagrams.md", dir)
            }
        };
        std::fs::write(&path, &content)?;
        println!("Mermaid diagrams saved to {}", path);
        if print_content {
            println!("\n{}", content);
        }
    }

    Ok(())
}
