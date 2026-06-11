//! Project diagram generation (Mermaid and draw.io formats).
//!
//! Analyzes project structure, source files, and configuration to generate
//! architecture diagrams, API route maps, and database schema visualizations.

pub mod api_routes;
pub mod architecture;
pub mod db_models;
pub mod drawio;
pub mod graph_html;
pub mod ir;
pub mod service_detection;

use crate::model::Project;
use ir::DiagramIr;

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

/// Generate all Mermaid diagrams for a project (scans once via the IR).
pub fn generate_all(project: &Project) -> ProjectDiagrams {
    generate_all_from_ir(&ir::build_ir(project))
}

/// Render all Mermaid diagrams from a pre-built IR (shared with draw.io).
pub fn generate_all_from_ir(ir: &DiagramIr) -> ProjectDiagrams {
    ProjectDiagrams {
        architecture: architecture::render(ir),
        api_routes: api_routes::render_mermaid(&ir.routes),
        db_models: db_models::render_mermaid(&ir.models, &ir.model_links),
        warnings: ir.warnings.clone(),
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

    /// Parity: from ONE synthetic IR, both renderers must surface every
    /// element — services, labeled edges, routes (public + internal,
    /// summary + handler), model fields, FK links, externals, crates, and
    /// warnings. Any future renderer drift fails here.
    #[test]
    fn test_renderer_parity_from_one_ir() {
        use super::api_routes::Route;
        use super::db_models::DbModel;
        use super::ir::*;
        use super::service_detection::ServiceType;
        use crate::docker::DockerAnalysis;

        let models = vec![
            DbModel {
                name: "Widget".into(),
                fields: vec![
                    ("wid_pk".into(), "int".into()),
                    ("owner_id".into(), "FK".into()),
                    ("label_txt".into(), "string".into()),
                ],
            },
            DbModel {
                name: "Owner".into(),
                fields: vec![
                    ("own_pk".into(), "int".into()),
                    ("nickname".into(), "string".into()),
                ],
            },
        ];
        let model_links = compute_model_links(&models);
        assert_eq!(model_links.len(), 1, "owner_id FK must resolve to Owner");

        let mk_route = |method: &str, path: &str, handler: &str, summary: Option<&str>| Route {
            method: method.into(),
            path: path.into(),
            handler: handler.into(),
            tag: None,
            summary: summary.map(|s| s.to_string()),
            internal: path.contains("/internal"),
        };

        let ir = DiagramIr {
            project_name: "parity-proj".into(),
            services: vec![
                ServiceNode {
                    name: "webfront".into(),
                    service_type: ServiceType::Frontend,
                    port: Some(3000),
                    command: "npm run dev".into(),
                },
                ServiceNode {
                    name: "apicore".into(),
                    service_type: ServiceType::Backend,
                    port: Some(8080),
                    command: "go run .".into(),
                },
                ServiceNode {
                    name: "pgstore".into(),
                    service_type: ServiceType::Database,
                    port: None,
                    command: "postgres".into(),
                },
            ],
            externals: vec!["SentinelExt".into()],
            crate_links: vec![("crate_alpha".into(), "crate_beta".into())],
            edges: vec![
                ArchEdge {
                    from: "webfront".into(),
                    to: "apicore".into(),
                    label: Some("API".into()),
                    kind: ArchEdgeKind::Api,
                },
                ArchEdge {
                    from: "apicore".into(),
                    to: "SentinelExt".into(),
                    label: None,
                    kind: ArchEdgeKind::External,
                },
                ArchEdge {
                    from: "webfront".into(),
                    to: "apicore".into(),
                    label: Some("grpc: AuthService.Login".into()),
                    kind: ArchEdgeKind::Contract,
                },
            ],
            infra: DockerAnalysis {
                has_dockerfile: false,
                has_compose: false,
                dockerfile: None,
                compose: None,
                terraform: vec![],
                kubernetes: vec![],
                helm: None,
            },
            routes: vec![(
                "apicore".into(),
                vec![
                    mk_route("GET", "/v1/widgets", "list_widgets", None),
                    mk_route(
                        "POST",
                        "/v1/widgets",
                        "create_widget",
                        Some("Create a widget"),
                    ),
                    mk_route("GET", "/internal/health", "health_check", None),
                ],
            )],
            models,
            model_links,
            warnings: vec!["sentinel warning one".into(), "sentinel warning two".into()],
        };

        let mermaid = generate_all_from_ir(&ir);
        let dio = drawio::render_all_from_ir(&ir);

        let mermaid_all = format!(
            "{}\n{}\n{}",
            mermaid.architecture,
            mermaid.api_routes.as_deref().expect("mermaid api page"),
            mermaid.db_models.as_deref().expect("mermaid db page"),
        );
        assert!(dio.api_routes.is_some(), "drawio api page");
        assert!(dio.db_models.is_some(), "drawio db page");
        let drawio_all = dio.combined.clone();

        for (fmt, out) in [("mermaid", &mermaid_all), ("drawio", &drawio_all)] {
            // N services
            for svc in ["webfront", "apicore", "pgstore"] {
                assert!(out.contains(svc), "{fmt}: missing service {svc}");
            }
            // Edge labels (incl. the contract edge)
            for label in ["API", "grpc: AuthService.Login", "dep"] {
                assert!(out.contains(label), "{fmt}: missing edge label {label}");
            }
            // K routes: handler fallback, summary, and internal split
            assert_eq!(
                out.matches("/v1/widgets").count(),
                2,
                "{fmt}: public route count"
            );
            assert_eq!(
                out.matches("/internal/health").count(),
                1,
                "{fmt}: internal route count"
            );
            for marker in ["list_widgets", "Create a widget", "Internal API"] {
                assert!(out.contains(marker), "{fmt}: missing route marker {marker}");
            }
            // F model fields + FK relationship between the models
            for field in ["wid_pk", "owner_id", "label_txt", "own_pk", "nickname"] {
                assert!(out.contains(field), "{fmt}: missing model field {field}");
            }
            // Externals and crates
            for node in ["SentinelExt", "crate_alpha", "crate_beta"] {
                assert!(out.contains(node), "{fmt}: missing node {node}");
            }
        }

        // M edges: ir.edges (3) + crate dep edge (1) on the architecture page.
        let mermaid_edges = mermaid
            .architecture
            .lines()
            .filter(|l| l.contains("-->") || l.contains("-.->"))
            .count();
        let drawio_edges = dio.architecture.matches("edge=\"1\"").count();
        assert_eq!(mermaid_edges, 4, "mermaid edge count");
        assert_eq!(drawio_edges, 4, "drawio edge count");

        // W warnings surfaced identically by both formats.
        assert_eq!(mermaid.warnings, ir.warnings);
        assert_eq!(dio.warnings, ir.warnings);
    }
}
