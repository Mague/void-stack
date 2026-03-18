//! Kubernetes subgraph generation.

use crate::docker;

use super::super::sanitize_id;

/// Generate Kubernetes subgraph and return node IDs for styling.
pub(crate) fn generate(analysis: &docker::DockerAnalysis, lines: &mut Vec<String>) -> Vec<String> {
    if analysis.kubernetes.is_empty() {
        return Vec::new();
    }

    let mut k8s_ids = Vec::new();

    lines.push("    subgraph k8s [\"Kubernetes\"]".to_string());
    for res in &analysis.kubernetes {
        let id = format!("k8s_{}_{}", sanitize_id(&res.kind), sanitize_id(&res.name));
        let extras = build_extras(res);

        let node = match res.kind.as_str() {
            "Deployment" | "StatefulSet" | "DaemonSet" => {
                format!("        {}[\"{}: {}{}\"]", id, res.kind, res.name, extras)
            }
            "Service" => {
                format!("        {}([\"{}: {}{}\"])", id, res.kind, res.name, extras)
            }
            "Ingress" => {
                format!(
                    "        {}>{{\"{}: {}{}\"}}]",
                    id, res.kind, res.name, extras
                )
            }
            _ => {
                format!("        {}[\"{}: {}\"]", id, res.kind, res.name)
            }
        };
        lines.push(node);
        k8s_ids.push(id);
    }
    lines.push("    end".to_string());

    // Service → Deployment connections
    let deployments: Vec<&docker::K8sResource> = analysis
        .kubernetes
        .iter()
        .filter(|r| r.kind == "Deployment" || r.kind == "StatefulSet")
        .collect();
    let services: Vec<&docker::K8sResource> = analysis
        .kubernetes
        .iter()
        .filter(|r| r.kind == "Service")
        .collect();
    for svc in &services {
        for deploy in &deployments {
            if svc.name.contains(&deploy.name) || deploy.name.contains(&svc.name) {
                let svc_id = format!("k8s_{}_{}", sanitize_id(&svc.kind), sanitize_id(&svc.name));
                let dep_id = format!(
                    "k8s_{}_{}",
                    sanitize_id(&deploy.kind),
                    sanitize_id(&deploy.name)
                );
                lines.push(format!("    {} --> {}", svc_id, dep_id));
            }
        }
    }

    k8s_ids
}

pub(crate) fn build_extras(res: &docker::K8sResource) -> String {
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
