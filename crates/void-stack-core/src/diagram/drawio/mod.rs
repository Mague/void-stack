//! Draw.io (.drawio) XML diagram generation.
//!
//! Generates multi-page architecture diagrams in draw.io format.
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

use crate::diagram;
use crate::model::Project;

/// Generate a multi-page draw.io file with architecture + API routes + DB models.
pub fn generate_all(project: &Project) -> String {
    let routes = diagram::api_routes::scan_raw(project);
    let models = diagram::db_models::scan_raw(project);

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<mxfile host=\"void-stack\" agent=\"void-stack\" version=\"1.0\">\n");

    architecture::generate_architecture_page(project, &mut xml);
    api_routes::render_api_routes_page(&routes, &mut xml);
    db_models::render_db_models_page(&models, &mut xml);

    xml.push_str("</mxfile>\n");
    xml
}

fn wrap_page(page_xml: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mxfile host=\"void-stack\" agent=\"void-stack\" version=\"1.0\">\n{}</mxfile>\n",
        page_xml
    )
}

/// Generate only the architecture diagram as a standalone Draw.io XML.
pub fn generate_architecture(project: &Project) -> String {
    let mut xml = String::new();
    architecture::generate_architecture_page(project, &mut xml);
    wrap_page(&xml)
}

/// Generate only the API routes diagram as a standalone Draw.io XML, if any.
pub fn generate_api_routes(project: &Project) -> Option<String> {
    let routes = diagram::api_routes::scan_raw(project);
    if routes.is_empty() {
        return None;
    }
    let mut xml = String::new();
    api_routes::render_api_routes_page(&routes, &mut xml);
    if xml.contains("mxCell") {
        Some(wrap_page(&xml))
    } else {
        None
    }
}

/// Generate only the DB models diagram as a standalone Draw.io XML, if any.
pub fn generate_db_models(project: &Project) -> Option<String> {
    let models = diagram::db_models::scan_raw(project);
    if models.is_empty() {
        return None;
    }
    let mut xml = String::new();
    db_models::render_db_models_page(&models, &mut xml);
    if xml.contains("mxCell") {
        Some(wrap_page(&xml))
    } else {
        None
    }
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
    fn test_generate_architecture() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());

        let xml = generate_architecture(&project);
        assert!(xml.contains("mxGraphModel"));
        assert!(xml.contains("api"));
    }

    #[test]
    fn test_generate_api_routes_none() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());
        assert!(generate_api_routes(&project).is_none());
    }

    #[test]
    fn test_generate_api_routes_with_routes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from fastapi import FastAPI\napp = FastAPI()\n\n@app.get(\"/users\")\ndef list_users():\n    pass\n\n@app.post(\"/users\")\ndef create_user():\n    pass\n",
        ).unwrap();

        let project = make_project(dir.path());
        let result = generate_api_routes(&project);
        assert!(result.is_some());
        let xml = result.unwrap();
        assert!(xml.contains("/users"));
        assert!(xml.contains("GET"));
        assert!(xml.contains("POST"));
    }

    #[test]
    fn test_generate_db_models_none() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());
        assert!(generate_db_models(&project).is_none());
    }

    #[test]
    fn test_generate_db_models_with_models() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("prisma")).unwrap();
        std::fs::write(
            dir.path().join("prisma/schema.prisma"),
            "model User {\n  id    Int    @id\n  name  String\n  email String\n}\n",
        )
        .unwrap();

        let project = make_project(dir.path());
        let result = generate_db_models(&project);
        assert!(result.is_some());
        assert!(result.unwrap().contains("User"));
    }

    #[test]
    fn test_add_if() {
        let mut list = Vec::new();
        architecture::tests_helper_add_if(
            &mut list,
            "image: postgres:16",
            "postgres",
            "PostgreSQL",
        );
        architecture::tests_helper_add_if(
            &mut list,
            "image: postgres:16",
            "postgres",
            "PostgreSQL",
        );
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_detect_external_services_from_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "REDIS_URL=redis://localhost\n").unwrap();

        let project = make_project(dir.path());
        let externals = architecture::tests_helper_detect_externals(dir.path(), &project);
        assert!(externals.iter().any(|e| e == "Redis"));
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
