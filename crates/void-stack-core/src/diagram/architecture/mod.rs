//! Service architecture diagram generator.
//!
//! Generates Mermaid diagrams showing services, connections, external deps,
//! Rust crate relationships, and infrastructure (Terraform, K8s, Helm).

mod externals;
mod crates;
mod infra;

use std::path::Path;

use crate::docker;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

use super::service_detection::{self, ServiceType};

/// Generate a Mermaid architecture diagram for a project's services.
pub fn generate(project: &Project) -> String {
    let mut lines = vec![
        "```mermaid".to_string(),
        "graph TB".to_string(),
        format!("    subgraph proj_{} [\"{}\" ]", sanitize_id(&project.name), project.name),
    ];

    let mut connections: Vec<(String, String)> = Vec::new();

    for svc in &project.services {
        let id = sanitize_id(&svc.name);
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);

        let (svc_type, port) = service_detection::detect_service_info(dir_path, &svc.command);
        let icon = match svc_type {
            ServiceType::Frontend => "🌐",
            ServiceType::Backend => "⚙️",
            ServiceType::Database => "🗄️",
            ServiceType::Worker => "⚡",
            ServiceType::Unknown => "📦",
        };

        let port_label = port
            .map(|p| format!(" :{}",p))
            .unwrap_or_default();

        lines.push(format!(
            "        {}[\"{} {}{}<br/>{}\"]",
            id, icon, svc.name, port_label,
            match svc_type {
                ServiceType::Frontend => "Frontend",
                ServiceType::Backend => "API",
                ServiceType::Database => "Database",
                ServiceType::Worker => "Worker",
                ServiceType::Unknown => &svc.command,
            }
        ));

        if matches!(svc_type, ServiceType::Frontend) {
            for other in &project.services {
                let other_dir = other.working_dir.as_deref().unwrap_or(&project.path);
                let other_dir_clean = strip_win_prefix(other_dir);
                let other_path = Path::new(&other_dir_clean);
                let (other_type, _) = service_detection::detect_service_info(other_path, &other.command);
                if matches!(other_type, ServiceType::Backend) {
                    connections.push((id.clone(), sanitize_id(&other.name)));
                }
            }
        }
    }

    lines.push("    end".to_string());

    // External services
    let root = strip_win_prefix(&project.path);
    let root_path = Path::new(&root);
    let ext_services = externals::detect_external_services(root_path, project);
    for ext in &ext_services {
        lines.push(format!("    {}[(\"{}\")]", sanitize_id(ext), ext));
    }

    // Rust crate relationships
    let crate_links = crates::detect_crate_relationships(root_path);
    if !crate_links.is_empty() {
        lines.push(format!("    subgraph crates [\"Rust Crates\"]"));
        let mut crate_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (from, to) in &crate_links {
            crate_names.insert(from);
            crate_names.insert(to);
        }
        for name in &crate_names {
            let cid = format!("crate_{}", sanitize_id(name));
            lines.push(format!("        {}[\"📦 {}\"]", cid, name));
        }
        lines.push("    end".to_string());
        for (from, to) in &crate_links {
            let fid = format!("crate_{}", sanitize_id(from));
            let tid = format!("crate_{}", sanitize_id(to));
            lines.push(format!("    {} -->|dep| {}", fid, tid));
        }
    }

    // Connections
    for (from, to) in &connections {
        lines.push(format!("    {} -->|API| {}", from, to));
    }
    for ext in &ext_services {
        let ext_id = sanitize_id(ext);
        for svc in &project.services {
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let dir_stripped = strip_win_prefix(dir);
            let dir_path = Path::new(&dir_stripped);
            let (svc_type, _) = service_detection::detect_service_info(dir_path, &svc.command);
            if matches!(svc_type, ServiceType::Backend) {
                lines.push(format!("    {} -.-> {}", sanitize_id(&svc.name), ext_id));
            }
        }
    }

    // Infrastructure
    let docker_analysis = docker::analyze_docker(root_path);
    let infra_node_ids = infra::generate_infra_subgraphs(&docker_analysis, &mut lines);

    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_stripped = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_stripped);
        let (svc_type, _) = service_detection::detect_service_info(dir_path, &svc.command);
        if matches!(svc_type, ServiceType::Backend) {
            for infra_id in &infra_node_ids {
                lines.push(format!("    {} -.-> {}", sanitize_id(&svc.name), infra_id));
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

    for svc in &project.services {
        let id = sanitize_id(&svc.name);
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let (svc_type, _) = service_detection::detect_service_info(Path::new(&strip_win_prefix(dir)), &svc.command);
        let class = match svc_type {
            ServiceType::Frontend => "frontend",
            ServiceType::Backend => "backend",
            ServiceType::Database => "database",
            _ => "backend",
        };
        lines.push(format!("    class {} {}", id, class));
    }
    for ext in &ext_services {
        lines.push(format!("    class {} external", sanitize_id(ext)));
    }
    for (from, to) in &crate_links {
        lines.push(format!("    class crate_{} crate", sanitize_id(from)));
        lines.push(format!("    class crate_{} crate", sanitize_id(to)));
    }

    lines.push("```".to_string());
    lines.join("\n")
}

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
