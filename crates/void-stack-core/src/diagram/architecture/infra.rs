//! Infrastructure diagram generation (Terraform, Kubernetes, Helm).

use crate::docker;

use super::sanitize_id;

/// Generate Mermaid subgraphs for Terraform, Kubernetes, and Helm resources.
/// Returns a list of node IDs for infrastructure resources (used for connections).
pub(super) fn generate_infra_subgraphs(analysis: &docker::DockerAnalysis, lines: &mut Vec<String>) -> Vec<String> {
    let mut infra_ids = Vec::new();

    // Terraform resources
    if !analysis.terraform.is_empty() {
        lines.push("    subgraph infra [\"Infrastructure (Terraform)\"]".to_string());
        for res in &analysis.terraform {
            let id = format!("tf_{}_{}", sanitize_id(&res.provider), sanitize_id(&res.name));
            let details = if res.details.is_empty() {
                String::new()
            } else {
                format!("<br/>{}", res.details.join(", "))
            };

            let node = match res.kind {
                docker::InfraResourceKind::Database => {
                    format!("        {}[(\"{} {}{}\")]", id, res.resource_type, res.name, details)
                }
                docker::InfraResourceKind::Compute => {
                    format!("        {}{{\"{} {}{}\"}}", id, res.resource_type, res.name, details)
                }
                docker::InfraResourceKind::Storage => {
                    format!("        {}[/\"{} {}{}\"/]", id, res.resource_type, res.name, details)
                }
                docker::InfraResourceKind::Queue => {
                    format!("        {}[[\"{} {}{}\"]]", id, res.resource_type, res.name, details)
                }
                _ => {
                    format!("        {}[\"{} {}{}\"]", id, res.resource_type, res.name, details)
                }
            };
            lines.push(node);

            let class = match res.kind {
                docker::InfraResourceKind::Database => "infra_db",
                docker::InfraResourceKind::Cache => "infra_cache",
                docker::InfraResourceKind::Storage => "infra_storage",
                docker::InfraResourceKind::Compute => "infra_compute",
                docker::InfraResourceKind::Queue => "infra_queue",
                _ => "external",
            };
            infra_ids.push(format!("{}:{}", id, class));
        }
        lines.push("    end".to_string());
    }

    // Kubernetes resources
    if !analysis.kubernetes.is_empty() {
        lines.push("    subgraph k8s [\"Kubernetes\"]".to_string());
        for res in &analysis.kubernetes {
            let id = format!("k8s_{}_{}", sanitize_id(&res.kind), sanitize_id(&res.name));
            let extras = build_k8s_extras(res);

            let node = match res.kind.as_str() {
                "Deployment" | "StatefulSet" | "DaemonSet" => {
                    format!("        {}[\"{}: {}{}\"]", id, res.kind, res.name, extras)
                }
                "Service" => {
                    format!("        {}([\"{}: {}{}\"])", id, res.kind, res.name, extras)
                }
                "Ingress" => {
                    format!("        {}>{{\"{}: {}{}\"}}]", id, res.kind, res.name, extras)
                }
                _ => {
                    format!("        {}[\"{}: {}\"]", id, res.kind, res.name)
                }
            };
            lines.push(node);
        }
        lines.push("    end".to_string());

        // K8s Service → Deployment connections
        let deployments: Vec<&docker::K8sResource> = analysis.kubernetes.iter()
            .filter(|r| r.kind == "Deployment" || r.kind == "StatefulSet")
            .collect();
        let services: Vec<&docker::K8sResource> = analysis.kubernetes.iter()
            .filter(|r| r.kind == "Service")
            .collect();
        for svc in &services {
            for deploy in &deployments {
                if svc.name.contains(&deploy.name) || deploy.name.contains(&svc.name) {
                    let svc_id = format!("k8s_{}_{}", sanitize_id(&svc.kind), sanitize_id(&svc.name));
                    let dep_id = format!("k8s_{}_{}", sanitize_id(&deploy.kind), sanitize_id(&deploy.name));
                    lines.push(format!("    {} --> {}", svc_id, dep_id));
                }
            }
        }
    }

    // Helm chart
    if let Some(ref chart) = analysis.helm {
        lines.push(format!("    subgraph helm_chart [\"Helm: {} v{}\"]", chart.name, chart.version));
        for dep in &chart.dependencies {
            let id = format!("helm_{}", sanitize_id(&dep.name));
            lines.push(format!("        {}[\"{} ({})\"]", id, dep.name, dep.version));
        }
        lines.push("    end".to_string());
    }

    // Apply styling classes
    let mut result_ids = Vec::new();
    for entry in &infra_ids {
        if let Some((id, class)) = entry.split_once(':') {
            lines.push(format!("    class {} {}", id, class));
            result_ids.push(id.to_string());
        }
    }

    for res in &analysis.kubernetes {
        let id = format!("k8s_{}_{}", sanitize_id(&res.kind), sanitize_id(&res.name));
        lines.push(format!("    class {} k8s", id));
    }

    if let Some(ref chart) = analysis.helm {
        for dep in &chart.dependencies {
            let id = format!("helm_{}", sanitize_id(&dep.name));
            lines.push(format!("    class {} helm", id));
        }
    }

    result_ids
}

fn build_k8s_extras(res: &docker::K8sResource) -> String {
    let mut parts = Vec::new();
    if let Some(r) = res.replicas {
        parts.push(format!("x{}", r));
    }
    if !res.images.is_empty() {
        parts.push(res.images.join(", "));
    }
    if !res.ports.is_empty() {
        let ports: Vec<String> = res.ports.iter().map(|p| p.to_string()).collect();
        parts.push(format!(":{}", ports.join(",")));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("<br/>{}", parts.join(" | "))
    }
}
