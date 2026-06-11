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
