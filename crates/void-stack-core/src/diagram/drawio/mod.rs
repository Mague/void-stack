//! Draw.io (.drawio) XML diagram generation.
//!
//! Renders multi-page architecture diagrams from the shared [`DiagramIr`] —
//! the same IR instance the Mermaid renderer consumes, so both formats stay
//! in parity by construction.
//!
//! Split into submodules:
//! - `common` — shared constants, IdGen, XML escaping
//! - `architecture` — service architecture page
//! - `api_routes` — API routes page
//! - `db_models` — database models page with FK-proximity layout

mod api_routes;
mod architecture;
mod common;
mod db_models;

use crate::model::Project;

use super::ir::{self, DiagramIr};

/// All draw.io pages rendered from one IR, plus the combined multi-page file
/// and the IR-level warnings (same warnings the Mermaid path surfaces).
pub struct DrawioDiagrams {
    pub architecture: String,
    pub api_routes: Option<String>,
    pub db_models: Option<String>,
    /// Multi-page .drawio file with every page.
    pub combined: String,
    pub warnings: Vec<String>,
}

/// Render every draw.io page from a pre-built IR.
pub fn render_all_from_ir(ir: &DiagramIr) -> DrawioDiagrams {
    let mut arch_xml = String::new();
    architecture::generate_architecture_page(ir, &mut arch_xml);

    let mut api_xml = String::new();
    api_routes::render_api_routes_page(&ir.routes, &mut api_xml);

    let mut db_xml = String::new();
    db_models::render_db_models_page(&ir.models, &ir.model_links, &mut db_xml);

    let mut combined = String::new();
    combined.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    combined.push_str("<mxfile host=\"void-stack\" agent=\"void-stack\" version=\"1.0\">\n");
    combined.push_str(&arch_xml);
    combined.push_str(&api_xml);
    combined.push_str(&db_xml);
    combined.push_str("</mxfile>\n");

    DrawioDiagrams {
        architecture: wrap_page(&arch_xml),
        api_routes: page_if_content(&api_xml),
        db_models: page_if_content(&db_xml),
        combined,
        warnings: ir.warnings.clone(),
    }
}

/// Generate the combined multi-page draw.io file (scans once via the IR).
pub fn generate_all(project: &Project) -> String {
    render_all_from_ir(&ir::build_ir(project)).combined
}

fn page_if_content(page_xml: &str) -> Option<String> {
    if page_xml.contains("mxCell") {
        Some(wrap_page(page_xml))
    } else {
        None
    }
}

fn wrap_page(page_xml: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mxfile host=\"void-stack\" agent=\"void-stack\" version=\"1.0\">\n{}</mxfile>\n",
        page_xml
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, Service, Target};

    fn make_service(name: &str, command: &str, dir: &std::path::Path) -> Service {
        Service {
            name: name.to_string(),
            command: command.to_string(),
            target: Target::Windows,
            working_dir: Some(dir.to_string_lossy().to_string()),
            enabled: true,
            env_vars: Vec::new(),
            depends_on: Vec::new(),
            docker: None,
        }
    }

    fn make_project(dir: &std::path::Path) -> Project {
        Project {
            name: "test-project".to_string(),
            description: String::new(),
            path: dir.to_string_lossy().to_string(),
            project_type: None,
            tags: Vec::new(),
            services: vec![make_service("api", "npm start", dir)],
            hooks: None,
        }
    }

    #[test]
    fn test_esc() {
        assert_eq!(common::esc("hello"), "hello");
        assert_eq!(common::esc("<b>bold</b>"), "&lt;b&gt;bold&lt;/b&gt;");
        assert_eq!(common::esc("a & b"), "a &amp; b");
        assert_eq!(common::esc(r#"say "hi""#), "say &quot;hi&quot;");
    }

    #[test]
    fn test_id_gen() {
        let mut id_gen = common::IdGen::new();
        assert_eq!(id_gen.next(), 2);
        assert_eq!(id_gen.next(), 3);
        assert_eq!(id_gen.next(), 4);
    }

    #[test]
    fn test_generate_all_structure() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());

        let xml = generate_all(&project);
        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("<mxfile"));
        assert!(xml.contains("</mxfile>"));
        assert!(xml.contains("Architecture"));
        assert!(xml.contains("test-project"));
    }

    #[test]
    fn test_render_all_from_ir_pages() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());

        let ir = ir::build_ir(&project);
        let pages = render_all_from_ir(&ir);
        assert!(pages.architecture.contains("mxGraphModel"));
        assert!(pages.architecture.contains("api"));
        // No routes/models in an empty temp project.
        assert!(pages.api_routes.is_none());
        assert!(pages.db_models.is_none());
        assert!(pages.warnings.is_empty());
    }

    #[test]
    fn test_api_routes_page_with_routes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from fastapi import FastAPI\napp = FastAPI()\n\n@app.get(\"/users\")\ndef list_users():\n    pass\n\n@app.post(\"/users\")\ndef create_user():\n    pass\n",
        ).unwrap();

        let project = make_project(dir.path());
        let pages = render_all_from_ir(&ir::build_ir(&project));
        let xml = pages.api_routes.expect("routes page");
        assert!(xml.contains("/users"));
        assert!(xml.contains("GET"));
        assert!(xml.contains("POST"));
        // Handler names now appear when no swagger summary exists.
        assert!(xml.contains("list_users"));
    }

    #[test]
    fn test_db_models_page_with_models() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("prisma")).unwrap();
        std::fs::write(
            dir.path().join("prisma/schema.prisma"),
            "model User {\n  id    Int    @id\n  name  String\n  email String\n}\n",
        )
        .unwrap();

        let project = make_project(dir.path());
        let pages = render_all_from_ir(&ir::build_ir(&project));
        assert!(pages.db_models.expect("db page").contains("User"));
    }

    #[test]
    fn test_generate_all_multi_service() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: "multi".to_string(),
            description: String::new(),
            path: dir.path().to_string_lossy().to_string(),
            project_type: None,
            tags: Vec::new(),
            services: vec![
                make_service("frontend", "npm start", dir.path()),
                make_service("backend", "python main.py", dir.path()),
            ],
            hooks: None,
        };

        let xml = generate_all(&project);
        assert!(xml.contains("frontend"));
        assert!(xml.contains("backend"));
    }
}
