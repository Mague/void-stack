//! Project diagram generation (Mermaid and draw.io formats).
//!
//! Analyzes project structure, source files, and configuration to generate
//! architecture diagrams, API route maps, and database schema visualizations.

pub mod architecture;
pub mod api_routes;
pub mod db_models;
pub mod drawio;

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
}

/// Generate all diagrams for a project.
pub fn generate_all(project: &Project) -> ProjectDiagrams {
    let arch = architecture::generate(project);
    let api = api_routes::generate(project);
    let db = db_models::generate(project);

    ProjectDiagrams {
        architecture: arch,
        api_routes: if api.contains("GET\\|POST\\|PUT\\|DELETE") || api.lines().count() > 4 {
            Some(api)
        } else {
            // Check if there's actual content beyond the wrapper
            let content_lines: Vec<&str> = api.lines()
                .filter(|l| !l.trim().is_empty() && !l.contains("graph") && !l.contains("```"))
                .collect();
            if content_lines.len() > 1 {
                Some(api)
            } else {
                None
            }
        },
        db_models: if db.lines().count() > 4 { Some(db) } else { None },
    }
}
