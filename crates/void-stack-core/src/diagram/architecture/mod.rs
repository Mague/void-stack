//! Service architecture diagram generator (Mermaid renderer).
//!
//! Renders services, connections, external deps, Rust crate relationships,
//! and infrastructure (Terraform, K8s, Helm) from a pre-built [`DiagramIr`].
//! Scanners live behind [`detect_externals`]/[`detect_crates`] and are only
//! invoked by `ir::build_ir` — never by renderers.

mod crates;
mod externals;
mod infra;

use std::path::Path;

use crate::model::Project;

use super::ir::{ArchEdgeKind, DiagramIr};
// Re-imported for child modules (externals, infra) that reach them via `super::`.
use super::ir::sanitize_id;
use super::service_detection::{self, ServiceType};

/// Scanner entry point used by `ir::build_ir` (external services).
pub(in crate::diagram) fn detect_externals(root: &Path, project: &Project) -> Vec<String> {
    externals::detect_external_services(root, project)
}

/// Scanner entry point used by `ir::build_ir` (internal crate deps).
pub(in crate::diagram) fn detect_crates(root: &Path) -> Vec<(String, String)> {
    crates::detect_crate_relationships(root)
}

/// Render the Mermaid architecture diagram from the shared IR.
pub(in crate::diagram) fn render(ir: &DiagramIr) -> String {
    let mut lines = vec![
        "```mermaid".to_string(),
        "graph TB".to_string(),
        format!(
            "    subgraph proj_{} [\"{}\" ]",
            sanitize_id(&ir.project_name),
            ir.project_name
        ),
    ];

    for svc in &ir.services {
        let id = sanitize_id(&svc.name);
        let icon = match svc.service_type {
            ServiceType::Frontend => "🌐",
            ServiceType::Backend => "⚙️",
            ServiceType::Database => "🗄️",
            ServiceType::Worker => "⚡",
            ServiceType::Unknown => "📦",
        };
        let port_label = svc.port.map(|p| format!(" :{}", p)).unwrap_or_default();
        lines.push(format!(
            "        {}[\"{} {}{}<br/>{}\"]",
            id,
            icon,
            svc.name,
            port_label,
            match svc.service_type {
                ServiceType::Frontend => "Frontend",
                ServiceType::Backend => "API",
                ServiceType::Database => "Database",
                ServiceType::Worker => "Worker",
                ServiceType::Unknown => &svc.command,
            }
        ));
    }

    lines.push("    end".to_string());

    // External services
    for ext in &ir.externals {
        lines.push(format!("    {}[(\"{}\")]", sanitize_id(ext), ext));
    }

    // Rust crate relationships
    if !ir.crate_links.is_empty() {
        lines.push("    subgraph crates [\"Rust Crates\"]".to_string());
        let mut crate_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (from, to) in &ir.crate_links {
            crate_names.insert(from);
            crate_names.insert(to);
        }
        for name in &crate_names {
            let cid = format!("crate_{}", sanitize_id(name));
            lines.push(format!("        {}[\"📦 {}\"]", cid, name));
        }
        lines.push("    end".to_string());
        for (from, to) in &ir.crate_links {
            let fid = format!("crate_{}", sanitize_id(from));
            let tid = format!("crate_{}", sanitize_id(to));
            lines.push(format!("    {} -->|dep| {}", fid, tid));
        }
    }

    // Infrastructure subgraphs (node ids match the Infra edges in the IR).
    infra::generate_infra_subgraphs(&ir.infra, &mut lines);

    // Edges
    for edge in &ir.edges {
        let from = sanitize_id(&edge.from);
        match edge.kind {
            ArchEdgeKind::Api | ArchEdgeKind::Contract => {
                let label = edge.label.as_deref().unwrap_or("API");
                lines.push(format!(
                    "    {} -->|{}| {}",
                    from,
                    label,
                    sanitize_id(&edge.to)
                ));
            }
            ArchEdgeKind::External => {
                lines.push(format!("    {} -.-> {}", from, sanitize_id(&edge.to)));
            }
            // `to` is already a node id (e.g. tf_aws_main_db).
            ArchEdgeKind::Infra => {
                lines.push(format!("    {} -.-> {}", from, edge.to));
            }
        }
    }

    // Styling
    lines.push("".to_string());
    lines.push("    classDef frontend fill:#4CAF50,stroke:#333,color:#fff".to_string());
    lines.push("    classDef backend fill:#2196F3,stroke:#333,color:#fff".to_string());
    lines.push("    classDef database fill:#FF9800,stroke:#333,color:#fff".to_string());
    lines.push("    classDef external fill:#9E9E9E,stroke:#333,color:#fff".to_string());
    lines.push("    classDef crate fill:#E65100,stroke:#BF360C,color:#fff".to_string());
    lines.push("    classDef infra_db fill:#E91E63,stroke:#880E4F,color:#fff".to_string());
    lines.push("    classDef infra_cache fill:#FF5722,stroke:#BF360C,color:#fff".to_string());
    lines.push("    classDef infra_storage fill:#607D8B,stroke:#37474F,color:#fff".to_string());
    lines.push("    classDef infra_compute fill:#9C27B0,stroke:#4A148C,color:#fff".to_string());
    lines.push("    classDef infra_queue fill:#FFC107,stroke:#FF8F00,color:#000".to_string());
    lines.push("    classDef k8s fill:#326CE5,stroke:#1A3F7A,color:#fff".to_string());
    lines.push("    classDef helm fill:#0F1689,stroke:#091058,color:#fff".to_string());

    for svc in &ir.services {
        let class = match svc.service_type {
            ServiceType::Frontend => "frontend",
            ServiceType::Backend => "backend",
            ServiceType::Database => "database",
            _ => "backend",
        };
        lines.push(format!("    class {} {}", sanitize_id(&svc.name), class));
    }
    for ext in &ir.externals {
        lines.push(format!("    class {} external", sanitize_id(ext)));
    }
    for (from, to) in &ir.crate_links {
        lines.push(format!("    class crate_{} crate", sanitize_id(from)));
        lines.push(format!("    class crate_{} crate", sanitize_id(to)));
    }

    lines.push("```".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::ir::{ArchEdge, ServiceNode};
    use crate::docker::DockerAnalysis;
    use crate::model::Project;

    // ── Fixture helpers ─────────────────────────────────────────

    fn empty_ir(name: &str) -> DiagramIr {
        DiagramIr {
            project_name: name.to_string(),
            services: Vec::new(),
            externals: Vec::new(),
            crate_links: Vec::new(),
            edges: Vec::new(),
            infra: DockerAnalysis::default(),
            routes: Vec::new(),
            models: Vec::new(),
            model_links: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn service(name: &str, service_type: ServiceType, port: Option<u16>) -> ServiceNode {
        ServiceNode {
            name: name.to_string(),
            service_type,
            port,
            command: "custom-cmd".to_string(),
        }
    }

    fn make_project(path: &str) -> Project {
        Project {
            name: "fixture".to_string(),
            description: String::new(),
            path: path.to_string(),
            project_type: None,
            tags: Vec::new(),
            services: Vec::new(),
            hooks: None,
        }
    }

    // ── render ──────────────────────────────────────────────────

    #[test]
    fn test_render_empty_ir_has_mermaid_skeleton() {
        let out = render(&empty_ir("my-project"));
        assert!(out.starts_with("```mermaid"));
        assert!(out.ends_with("```"));
        assert!(out.contains("graph TB"));
        // Project subgraph uses the sanitized id and the raw name label.
        assert!(out.contains("subgraph proj_my_project [\"my-project\" ]"));
        // Style definitions are always emitted.
        assert!(out.contains("classDef frontend"));
        assert!(out.contains("classDef external"));
    }

    #[test]
    fn test_render_service_types_icons_and_classes() {
        let mut ir = empty_ir("p");
        ir.services = vec![
            service("web", ServiceType::Frontend, Some(3000)),
            service("api", ServiceType::Backend, Some(8080)),
            service("db", ServiceType::Database, None),
            service("jobs", ServiceType::Worker, None),
            service("misc", ServiceType::Unknown, None),
        ];
        let out = render(&ir);

        // Icons + type labels per service type.
        assert!(out.contains("web[\"🌐 web :3000<br/>Frontend\"]"));
        assert!(out.contains("api[\"⚙️ api :8080<br/>API\"]"));
        assert!(out.contains("db[\"🗄️ db<br/>Database\"]"));
        assert!(out.contains("jobs[\"⚡ jobs<br/>Worker\"]"));
        // Unknown services show the raw command instead of a type label.
        assert!(out.contains("misc[\"📦 misc<br/>custom-cmd\"]"));

        // Class assignments (Worker/Unknown fall back to backend).
        assert!(out.contains("class web frontend"));
        assert!(out.contains("class api backend"));
        assert!(out.contains("class db database"));
        assert!(out.contains("class jobs backend"));
        assert!(out.contains("class misc backend"));
    }

    #[test]
    fn test_render_externals_nodes_and_classes() {
        let mut ir = empty_ir("p");
        ir.externals = vec!["PostgreSQL".to_string(), "AWS S3".to_string()];
        let out = render(&ir);

        assert!(out.contains("PostgreSQL[(\"PostgreSQL\")]"));
        // Node id is sanitized, label keeps the original text.
        assert!(out.contains("AWS_S3[(\"AWS S3\")]"));
        assert!(out.contains("class PostgreSQL external"));
        assert!(out.contains("class AWS_S3 external"));
    }

    #[test]
    fn test_render_crate_links_subgraph_and_edges() {
        let mut ir = empty_ir("p");
        ir.crate_links = vec![("my-cli".to_string(), "my-core".to_string())];
        let out = render(&ir);

        assert!(out.contains("subgraph crates [\"Rust Crates\"]"));
        assert!(out.contains("crate_my_cli[\"📦 my-cli\"]"));
        assert!(out.contains("crate_my_core[\"📦 my-core\"]"));
        assert!(out.contains("crate_my_cli -->|dep| crate_my_core"));
        assert!(out.contains("class crate_my_cli crate"));
        assert!(out.contains("class crate_my_core crate"));
    }

    #[test]
    fn test_render_no_crate_subgraph_without_links() {
        let out = render(&empty_ir("p"));
        assert!(!out.contains("Rust Crates"));
    }

    #[test]
    fn test_render_edge_kinds() {
        let mut ir = empty_ir("p");
        ir.edges = vec![
            ArchEdge {
                from: "web".to_string(),
                to: "api".to_string(),
                label: Some("API".to_string()),
                kind: ArchEdgeKind::Api,
            },
            ArchEdge {
                from: "app".to_string(),
                to: "backend".to_string(),
                label: Some("grpc: Greeter".to_string()),
                kind: ArchEdgeKind::Contract,
            },
            ArchEdge {
                from: "api".to_string(),
                to: "Redis".to_string(),
                label: None,
                kind: ArchEdgeKind::External,
            },
            ArchEdge {
                from: "api".to_string(),
                to: "tf_aws_main_db".to_string(),
                label: None,
                kind: ArchEdgeKind::Infra,
            },
        ];
        let out = render(&ir);

        assert!(out.contains("web -->|API| api"));
        assert!(out.contains("app -->|grpc: Greeter| backend"));
        assert!(out.contains("api -.-> Redis"));
        // Infra edges keep `to` verbatim (already a node id).
        assert!(out.contains("api -.-> tf_aws_main_db"));
    }

    #[test]
    fn test_render_api_edge_without_label_defaults_to_api() {
        let mut ir = empty_ir("p");
        ir.edges = vec![ArchEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            label: None,
            kind: ArchEdgeKind::Api,
        }];
        let out = render(&ir);
        assert!(out.contains("a -->|API| b"));
    }

    // ── Scanner entry points ────────────────────────────────────

    #[test]
    fn test_detect_externals_from_env_fixture() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "DATABASE_URL=postgres://localhost/db\nREDIS_URL=redis://localhost:6379\n",
        )
        .unwrap();

        let project = make_project(&dir.path().to_string_lossy());
        let externals = detect_externals(dir.path(), &project);

        assert!(externals.iter().any(|e| e == "PostgreSQL"));
        assert!(externals.iter().any(|e| e == "Redis"));
    }

    #[test]
    fn test_detect_externals_empty_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(&dir.path().to_string_lossy());
        assert!(detect_externals(dir.path(), &project).is_empty());
    }

    #[test]
    fn test_detect_crates_from_workspace_fixture() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"core\", \"cli\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("core")).unwrap();
        std::fs::write(
            dir.path().join("core/Cargo.toml"),
            "[package]\nname = \"fx-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("cli")).unwrap();
        std::fs::write(
            dir.path().join("cli/Cargo.toml"),
            "[package]\nname = \"fx-cli\"\nversion = \"0.1.0\"\n\n[dependencies]\nfx-core = { path = \"../core\" }\n",
        )
        .unwrap();

        let links = detect_crates(dir.path());
        assert_eq!(links, vec![("fx-cli".to_string(), "fx-core".to_string())]);
    }

    #[test]
    fn test_detect_crates_no_workspace() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_crates(dir.path()).is_empty());
    }
}
