//! Intermediate representation for project diagrams.
//!
//! All scanners run exactly once in [`build_ir`]; every renderer (Mermaid,
//! draw.io) consumes the same [`DiagramIr`] instance. Renderers must never
//! call a scanner directly — that is what kept the two formats drifting
//! apart.

use std::path::Path;

use crate::docker::{self, DockerAnalysis};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
#[cfg(feature = "vector")]
use crate::vector_index::contracts::{self, ContractKind, ContractRole};

use super::api_routes::{self, Route};
use super::architecture;
use super::db_models::{self, DbModel};
use super::service_detection::{self, ServiceType};

/// A detected service with its classification.
pub struct ServiceNode {
    pub name: String,
    pub service_type: ServiceType,
    pub port: Option<u16>,
    pub command: String,
}

/// How an [`ArchEdge`] should be interpreted by renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchEdgeKind {
    /// Service → service (frontend calls backend).
    Api,
    /// Service → external dependency (`to` is an external name).
    External,
    /// Service → infrastructure node (`to` is an infra node id like
    /// `tf_aws_main_db`).
    Infra,
    /// Service → service edge derived from API-contract matching
    /// (gRPC/REST producer↔consumer inside the project).
    Contract,
}

/// An architecture-level edge between two named nodes.
pub struct ArchEdge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub kind: ArchEdgeKind,
}

/// A foreign-key style relationship between two models (indexes into
/// `DiagramIr::models`).
pub struct ModelLink {
    pub from: usize,
    pub to: usize,
    pub field: String,
}

/// Everything the renderers need, produced by one scanner pass.
pub struct DiagramIr {
    pub project_name: String,
    pub services: Vec<ServiceNode>,
    pub externals: Vec<String>,
    /// Internal Rust crate dependency pairs (from, to).
    pub crate_links: Vec<(String, String)>,
    pub edges: Vec<ArchEdge>,
    /// Docker/Terraform/K8s/Helm analysis (already structured).
    pub infra: DockerAnalysis,
    /// Routes grouped per service.
    pub routes: Vec<(String, Vec<Route>)>,
    pub models: Vec<DbModel>,
    pub model_links: Vec<ModelLink>,
    pub warnings: Vec<String>,
}

/// Run every scanner once and assemble the shared IR.
pub fn build_ir(project: &Project) -> DiagramIr {
    let root = strip_win_prefix(&project.path);
    let root_path = Path::new(&root);

    // Services.
    let mut services = Vec::new();
    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let (service_type, port) =
            service_detection::detect_service_info(Path::new(&dir_clean), &svc.command);
        services.push(ServiceNode {
            name: svc.name.clone(),
            service_type,
            port,
            command: svc.command.clone(),
        });
    }

    let externals = architecture::detect_externals(root_path, project);
    let crate_links = architecture::detect_crates(root_path);
    let infra = docker::analyze_docker(root_path);
    let (routes, skipped) = api_routes::scan_project(project);
    let models = db_models::scan_raw(project);
    let model_links = compute_model_links(&models);

    let mut edges = Vec::new();

    // Frontend → backend.
    for f in services
        .iter()
        .filter(|s| s.service_type == ServiceType::Frontend)
    {
        for b in services
            .iter()
            .filter(|s| s.service_type == ServiceType::Backend)
        {
            edges.push(ArchEdge {
                from: f.name.clone(),
                to: b.name.clone(),
                label: Some("API".to_string()),
                kind: ArchEdgeKind::Api,
            });
        }
    }

    // Backend → external.
    for b in services
        .iter()
        .filter(|s| s.service_type == ServiceType::Backend)
    {
        for ext in &externals {
            edges.push(ArchEdge {
                from: b.name.clone(),
                to: ext.clone(),
                label: None,
                kind: ArchEdgeKind::External,
            });
        }
    }

    // Backend → terraform infra nodes (same id scheme the renderers use).
    for b in services
        .iter()
        .filter(|s| s.service_type == ServiceType::Backend)
    {
        for res in &infra.terraform {
            edges.push(ArchEdge {
                from: b.name.clone(),
                to: format!(
                    "tf_{}_{}",
                    sanitize_id(&res.provider),
                    sanitize_id(&res.name)
                ),
                label: None,
                kind: ArchEdgeKind::Infra,
            });
        }
    }

    // Contract edges (consumer service → producer service). Contract
    // extraction lives in vector_index, so it needs the "vector" feature.
    #[cfg(feature = "vector")]
    edges.extend(contract_edges(project, &services));

    let warnings = skipped
        .into_iter()
        .map(|(svc, reason)| format!("API routes ({}): {}", svc, reason))
        .collect();

    DiagramIr {
        project_name: project.name.clone(),
        services,
        externals,
        crate_links,
        edges,
        infra,
        routes,
        models,
        model_links,
        warnings,
    }
}

/// Compute FK-style links between models from field name/type heuristics.
/// Shared by both renderers (previously draw.io-only).
pub fn compute_model_links(models: &[DbModel]) -> Vec<ModelLink> {
    let name_to_idx: std::collections::HashMap<String, usize> = models
        .iter()
        .enumerate()
        .map(|(i, m)| (m.name.to_lowercase(), i))
        .collect();

    let mut links = Vec::new();
    for (idx, model) in models.iter().enumerate() {
        for (field_name, field_type) in &model.fields {
            if !is_fk_field(field_name, field_type) {
                continue;
            }
            let target = field_name
                .trim_end_matches("Id")
                .trim_end_matches("_id")
                .to_lowercase();
            if target.is_empty() {
                continue;
            }
            let target_idx = name_to_idx
                .get(&target)
                .or_else(|| name_to_idx.get(&format!("{}s", target)))
                .or_else(|| {
                    name_to_idx
                        .iter()
                        .find(|(k, _)| k.trim_end_matches('s') == target)
                        .map(|(_, v)| v)
                });
            if let Some(&tidx) = target_idx
                && tidx != idx
            {
                links.push(ModelLink {
                    from: idx,
                    to: tidx,
                    field: field_name.clone(),
                });
            }
        }
    }
    links
}

/// FK heuristic shared by renderers (icons/colors) and link computation.
pub fn is_fk_field(field_name: &str, field_type: &str) -> bool {
    field_type == "FK"
        || field_type == "M2M"
        || (field_type == "uuid" && (field_name.ends_with("Id") || field_name.ends_with("_id")))
}

/// Derive service→service edges from API contracts produced and consumed
/// inside the same project (e.g. a Flutter service calling its Go backend
/// over gRPC). Contracts are attributed to a service by the longest
/// matching working-dir prefix.
#[cfg(feature = "vector")]
fn contract_edges(project: &Project, services: &[ServiceNode]) -> Vec<ArchEdge> {
    if services.len() < 2 {
        return Vec::new(); // No pair to connect.
    }
    let root = strip_win_prefix(&project.path);

    // Service → project-relative dir prefix ("" = project root).
    let prefixes: Vec<(usize, String)> = project
        .services
        .iter()
        .enumerate()
        .map(|(i, svc)| {
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let dir_clean = strip_win_prefix(dir);
            let rel = dir_clean
                .strip_prefix(&root)
                .unwrap_or("")
                .trim_start_matches(['/', '\\'])
                .replace('\\', "/");
            (i, rel)
        })
        .collect();

    let attribute = |file: &str| -> Option<usize> {
        let file = file.replace('\\', "/");
        prefixes
            .iter()
            .filter(|(_, p)| p.is_empty() || file.starts_with(&format!("{}/", p)))
            .max_by_key(|(_, p)| p.len())
            .map(|(i, _)| *i)
    };

    // key → (producer service idxs, consumer service idxs)
    let mut by_key: std::collections::BTreeMap<String, (ContractKind, Vec<usize>, Vec<usize>)> =
        std::collections::BTreeMap::new();
    for c in contracts::project_contracts(project) {
        if c.kind == ContractKind::GrpcProtoHash {
            continue; // File-identity contracts carry no endpoint label.
        }
        let Some(svc_idx) = attribute(&c.file) else {
            continue;
        };
        let entry = by_key
            .entry(c.key.clone())
            .or_insert_with(|| (c.kind, Vec::new(), Vec::new()));
        match c.role {
            ContractRole::Producer => entry.1.push(svc_idx),
            ContractRole::Consumer => entry.2.push(svc_idx),
        }
    }

    // (consumer, producer, kind) → keys
    let mut pairs: std::collections::BTreeMap<(usize, usize, &'static str), Vec<String>> =
        std::collections::BTreeMap::new();
    for (key, (kind, producers, consumers)) in &by_key {
        let prefix = match kind {
            ContractKind::Grpc => "grpc",
            ContractKind::Rest => "rest",
            ContractKind::GrpcProtoHash => continue,
        };
        for &c in consumers {
            for &p in producers {
                if c != p {
                    pairs.entry((c, p, prefix)).or_default().push(key.clone());
                }
            }
        }
    }

    pairs
        .into_iter()
        .map(|((c, p, prefix), mut keys)| {
            keys.sort();
            keys.dedup();
            let label = if keys.len() == 1 {
                format!("{}: {}", prefix, keys[0])
            } else {
                format!("{}: {} +{}", prefix, keys[0], keys.len() - 1)
            };
            ArchEdge {
                from: services[c].name.clone(),
                to: services[p].name.clone(),
                label: Some(label),
                kind: ArchEdgeKind::Contract,
            }
        })
        .collect()
}

pub(crate) fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, fields: &[(&str, &str)]) -> DbModel {
        DbModel {
            name: name.to_string(),
            fields: fields
                .iter()
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect(),
        }
    }

    #[test]
    fn test_compute_model_links_fk_and_plural() {
        let models = vec![
            model("Order", &[("id", "int"), ("user_id", "FK")]),
            model("User", &[("id", "int")]),
            model("Item", &[("id", "int"), ("orderId", "uuid")]),
        ];
        let links = compute_model_links(&models);
        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|l| l.from == 0 && l.to == 1));
        assert!(links.iter().any(|l| l.from == 2 && l.to == 0));
    }

    #[test]
    fn test_is_fk_field() {
        assert!(is_fk_field("user_id", "FK"));
        assert!(is_fk_field("tags", "M2M"));
        assert!(is_fk_field("orderId", "uuid"));
        assert!(!is_fk_field("name", "string"));
        assert!(!is_fk_field("created", "uuid"));
    }
}
