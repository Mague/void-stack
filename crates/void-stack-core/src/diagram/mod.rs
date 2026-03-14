//! Project diagram generation (Mermaid and draw.io formats).
//!
//! Analyzes project structure, source files, and configuration to generate
//! architecture diagrams, API route maps, and database schema visualizations.

pub mod api_routes;
pub mod architecture;
pub mod db_models;
pub mod drawio;
pub mod service_detection;

use crate::model::Project;

/// Supported output formats for diagrams.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiagramFormat {
    Mermaid,
    DrawIo,
}

/// All diagrams generated for a project.
pub struct ProjectDiagrams {
    /// Service architecture diagram.
    pub architecture: String,
    /// API routes diagram (if any detected).
    pub api_routes: Option<String>,
    /// Database models diagram (if any detected).
    pub db_models: Option<String>,
    /// Warnings about partial or failed parsing.
    pub warnings: Vec<String>,
}

/// Generate all diagrams for a project.
pub fn generate_all(project: &Project) -> ProjectDiagrams {
    let arch = architecture::generate(project);
    let api_result = api_routes::scan(project);
    let db = db_models::generate(project);

    let mut warnings = Vec::new();

    // Collect warnings from skipped API route scans
    for (svc, reason) in &api_result.skipped {
        warnings.push(format!("API routes ({}): {}", svc, reason));
    }

    let api = &api_result.diagram;
    let api_routes = if api.lines().count() > 4 {
        let content_lines: Vec<&str> = api
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.contains("graph") && !l.contains("```"))
            .collect();
        if content_lines.len() > 1 {
            Some(api.clone())
        } else {
            None
        }
    } else {
        None
    };

    let db_models = if db.lines().count() > 4 {
        Some(db)
    } else {
        None
    };

    // Add warnings for entirely missing sections
    if api_routes.is_none() && api_result.skipped.is_empty() {
        // No backend services at all
    }
    if db_models.is_none() {
        // DB models not detected is normal, no warning needed
    }

    ProjectDiagrams {
        architecture: arch,
        api_routes,
        db_models,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, Service, Target};

    #[test]
    fn test_diagram_format_eq() {
        assert_eq!(DiagramFormat::Mermaid, DiagramFormat::Mermaid);
        assert_ne!(DiagramFormat::Mermaid, DiagramFormat::DrawIo);
    }

    fn make_project() -> Project {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().replace('\\', "/");
        // Keep tempdir alive by leaking it (test only)
        let path_owned = path.clone();
        std::mem::forget(dir);
        Project {
            name: "test-proj".into(),
            description: "test".into(),
            path: path_owned,
            project_type: None,
            tags: vec![],
            services: vec![Service {
                name: "api".into(),
                command: "python main.py".into(),
                target: Target::Windows,
                working_dir: None,
                enabled: true,
                env_vars: vec![],
                depends_on: vec![],
                docker: None,
            }],
            hooks: None,
        }
    }

    #[test]
    fn test_generate_all_returns_architecture() {
        let project = make_project();
        let diagrams = generate_all(&project);
        assert!(diagrams.architecture.contains("```mermaid"));
        assert!(diagrams.architecture.contains("test-proj"));
    }

    #[test]
    fn test_generate_all_no_api_routes() {
        let project = make_project();
        let diagrams = generate_all(&project);
        // Without actual source files, no API routes should be detected
        assert!(diagrams.api_routes.is_none());
    }

    #[test]
    fn test_generate_all_no_db_models() {
        let project = make_project();
        let diagrams = generate_all(&project);
        assert!(diagrams.db_models.is_none());
    }
}
