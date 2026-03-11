//! Project diagram generation (Mermaid and draw.io formats).
//!
//! Analyzes project structure, source files, and configuration to generate
//! architecture diagrams, API route maps, and database schema visualizations.

pub mod architecture;
pub mod api_routes;
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
        let content_lines: Vec<&str> = api.lines()
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

    let db_models = if db.lines().count() > 4 { Some(db) } else { None };

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
