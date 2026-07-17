use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::runner::local::strip_win_prefix;

pub fn cmd_graph_html(project_name: &str) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let path = void_stack_core::diagram::graph_html::generate_graph_html(project, "en")
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("Graph generated: {}", path.display());
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil::{config_lock, isolate_data_dir, unique_name};
    use void_stack_core::global_config::{load_global_config, save_global_config};
    use void_stack_core::model::{Project, Service, Target};

    fn register_fixture(name: &str, root: &std::path::Path) {
        isolate_data_dir();
        let mut config = load_global_config().unwrap();
        config.projects.push(Project {
            name: name.to_string(),
            description: String::new(),
            path: root.to_string_lossy().into_owned(),
            project_type: None,
            tags: vec![],
            services: vec![Service {
                name: "api".into(),
                command: "cargo run".into(),
                target: Target::Windows,
                working_dir: Some(root.to_string_lossy().into_owned()),
                enabled: true,
                env_vars: vec![],
                depends_on: vec![],
                docker: None,
            }],
            hooks: None,
        });
        save_global_config(&config).unwrap();
    }

    #[test]
    fn test_cmd_diagram_not_found() {
        let _guard = config_lock();
        isolate_data_dir();
        let err = cmd_diagram("no-such-project-xyz", None, "mermaid", false).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn test_cmd_diagram_mermaid_writes_file() {
        let _guard = config_lock();
        let tmp = tempfile::tempdir().unwrap();
        let name = unique_name("diag-md");
        register_fixture(&name, tmp.path());

        let out = tmp.path().join("out.md");
        cmd_diagram(&name, Some(&out.to_string_lossy()), "mermaid", true).unwrap();

        let content = std::fs::read_to_string(&out).unwrap();
        assert!(content.contains("# "));
        assert!(content.contains("Service Architecture"));
    }

    #[test]
    fn test_cmd_diagram_drawio_writes_file() {
        let _guard = config_lock();
        let tmp = tempfile::tempdir().unwrap();
        let name = unique_name("diag-drawio");
        register_fixture(&name, tmp.path());

        let out = tmp.path().join("out.drawio");
        cmd_diagram(&name, Some(&out.to_string_lossy()), "drawio", false).unwrap();

        let content = std::fs::read_to_string(&out).unwrap();
        assert!(content.contains("mxGraphModel") || content.contains("<mxfile"));
    }

    #[test]
    fn test_cmd_graph_html_writes_into_project_out_dir() {
        let _guard = config_lock();
        let tmp = tempfile::tempdir().unwrap();
        // A real source file so the dependency graph has something to build.
        std::fs::write(
            tmp.path().join("main.rs"),
            "fn main() { helper(); }\nfn helper() {}\n",
        )
        .unwrap();
        let name = unique_name("graph-html");
        register_fixture(&name, tmp.path());

        cmd_graph_html(&name).unwrap();

        let expected = tmp.path().join("void-stack-out").join("graph.html");
        assert!(expected.is_file(), "graph.html should be generated");
    }
}
