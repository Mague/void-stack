//! Infrastructure diagram generation (Terraform, Kubernetes, Helm).

use crate::docker;

use super::sanitize_id;

/// Generate Mermaid subgraphs for Terraform, Kubernetes, and Helm resources.
/// Returns a list of node IDs for infrastructure resources (used for connections).
pub(super) fn generate_infra_subgraphs(
    analysis: &docker::DockerAnalysis,
    lines: &mut Vec<String>,
) -> Vec<String> {
    let mut infra_ids = Vec::new();

    // Terraform resources
    if !analysis.terraform.is_empty() {
        lines.push("    subgraph infra [\"Infrastructure (Terraform)\"]".to_string());
        for res in &analysis.terraform {
            let id = format!(
                "tf_{}_{}",
                sanitize_id(&res.provider),
                sanitize_id(&res.name)
            );
            let details = if res.details.is_empty() {
                String::new()
            } else {
                format!("<br/>{}", res.details.join(", "))
            };

            let node = match res.kind {
                docker::InfraResourceKind::Database => {
                    format!(
                        "        {}[(\"{} {}{}\")]",
                        id, res.resource_type, res.name, details
                    )
                }
                docker::InfraResourceKind::Compute => {
                    format!(
                        "        {}{{\"{} {}{}\"}}",
                        id, res.resource_type, res.name, details
                    )
                }
                docker::InfraResourceKind::Storage => {
                    format!(
                        "        {}[/\"{} {}{}\"/]",
                        id, res.resource_type, res.name, details
                    )
                }
                docker::InfraResourceKind::Queue => {
                    format!(
                        "        {}[[\"{} {}{}\"]]",
                        id, res.resource_type, res.name, details
                    )
                }
                _ => {
                    format!(
                        "        {}[\"{} {}{}\"]",
                        id, res.resource_type, res.name, details
                    )
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
        }
        lines.push("    end".to_string());

        // K8s Service → Deployment connections
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
                    let svc_id =
                        format!("k8s_{}_{}", sanitize_id(&svc.kind), sanitize_id(&svc.name));
                    let dep_id = format!(
                        "k8s_{}_{}",
                        sanitize_id(&deploy.kind),
                        sanitize_id(&deploy.name)
                    );
                    lines.push(format!("    {} --> {}", svc_id, dep_id));
                }
            }
        }
    }

    // Helm chart
    if let Some(ref chart) = analysis.helm {
        lines.push(format!(
            "    subgraph helm_chart [\"Helm: {} v{}\"]",
            chart.name, chart.version
        ));
        for dep in &chart.dependencies {
            let id = format!("helm_{}", sanitize_id(&dep.name));
            lines.push(format!(
                "        {}[\"{} ({})\"]",
                id, dep.name, dep.version
            ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::*;

    fn empty_analysis() -> DockerAnalysis {
        DockerAnalysis {
            has_dockerfile: false,
            has_compose: false,
            dockerfile: None,
            compose: None,
            terraform: vec![],
            kubernetes: vec![],
            helm: None,
        }
    }

    #[test]
    fn test_empty_infra() {
        let analysis = empty_analysis();
        let mut lines = Vec::new();
        let ids = generate_infra_subgraphs(&analysis, &mut lines);
        assert!(ids.is_empty());
        assert!(lines.is_empty());
    }

    #[test]
    fn test_terraform_database() {
        let mut analysis = empty_analysis();
        analysis.terraform.push(InfraResource {
            provider: "aws".into(),
            resource_type: "aws_db_instance".into(),
            name: "main_db".into(),
            kind: InfraResourceKind::Database,
            details: vec!["engine: postgres".into()],
        });
        let mut lines = Vec::new();
        let ids = generate_infra_subgraphs(&analysis, &mut lines);
        assert_eq!(ids.len(), 1);
        let joined = lines.join("\n");
        assert!(joined.contains("Infrastructure (Terraform)"));
        assert!(joined.contains("main_db"));
        assert!(joined.contains("engine: postgres"));
        assert!(joined.contains("class") && joined.contains("infra_db"));
    }

    #[test]
    fn test_terraform_compute() {
        let mut analysis = empty_analysis();
        analysis.terraform.push(InfraResource {
            provider: "aws".into(),
            resource_type: "aws_instance".into(),
            name: "web_server".into(),
            kind: InfraResourceKind::Compute,
            details: vec![],
        });
        let mut lines = Vec::new();
        generate_infra_subgraphs(&analysis, &mut lines);
        let joined = lines.join("\n");
        // Compute nodes use diamond shape {}
        assert!(joined.contains("{\"aws_instance web_server\"}"));
    }

    #[test]
    fn test_terraform_storage() {
        let mut analysis = empty_analysis();
        analysis.terraform.push(InfraResource {
            provider: "aws".into(),
            resource_type: "aws_s3_bucket".into(),
            name: "assets".into(),
            kind: InfraResourceKind::Storage,
            details: vec![],
        });
        let mut lines = Vec::new();
        generate_infra_subgraphs(&analysis, &mut lines);
        let joined = lines.join("\n");
        assert!(joined.contains("infra_storage"));
    }

    #[test]
    fn test_terraform_queue() {
        let mut analysis = empty_analysis();
        analysis.terraform.push(InfraResource {
            provider: "aws".into(),
            resource_type: "aws_sqs_queue".into(),
            name: "tasks".into(),
            kind: InfraResourceKind::Queue,
            details: vec![],
        });
        let mut lines = Vec::new();
        generate_infra_subgraphs(&analysis, &mut lines);
        let joined = lines.join("\n");
        assert!(joined.contains("infra_queue"));
    }

    #[test]
    fn test_kubernetes_deployment() {
        let mut analysis = empty_analysis();
        analysis.kubernetes.push(K8sResource {
            kind: "Deployment".into(),
            name: "api-server".into(),
            namespace: Some("default".into()),
            images: vec!["api:latest".into()],
            ports: vec![8080],
            replicas: Some(3),
        });
        let mut lines = Vec::new();
        generate_infra_subgraphs(&analysis, &mut lines);
        let joined = lines.join("\n");
        assert!(joined.contains("Kubernetes"));
        assert!(joined.contains("Deployment: api-server"));
        assert!(joined.contains("x3"));
    }

    #[test]
    fn test_kubernetes_service_to_deployment_connection() {
        let mut analysis = empty_analysis();
        analysis.kubernetes.push(K8sResource {
            kind: "Deployment".into(),
            name: "web".into(),
            namespace: None,
            images: vec![],
            ports: vec![],
            replicas: None,
        });
        analysis.kubernetes.push(K8sResource {
            kind: "Service".into(),
            name: "web".into(),
            namespace: None,
            images: vec![],
            ports: vec![80],
            replicas: None,
        });
        let mut lines = Vec::new();
        generate_infra_subgraphs(&analysis, &mut lines);
        let joined = lines.join("\n");
        assert!(joined.contains("k8s_Service_web --> k8s_Deployment_web"));
    }

    #[test]
    fn test_helm_chart() {
        let mut analysis = empty_analysis();
        analysis.helm = Some(HelmChart {
            name: "my-app".into(),
            version: "1.2.0".into(),
            dependencies: vec![HelmDependency {
                name: "postgresql".into(),
                version: "12.0.0".into(),
                repository: "https://charts.bitnami.com/bitnami".into(),
            }],
        });
        let mut lines = Vec::new();
        generate_infra_subgraphs(&analysis, &mut lines);
        let joined = lines.join("\n");
        assert!(joined.contains("Helm: my-app v1.2.0"));
        assert!(joined.contains("postgresql (12.0.0)"));
        assert!(joined.contains("class helm_postgresql helm"));
    }

    #[test]
    fn test_build_k8s_extras_empty() {
        let res = K8sResource {
            kind: "Deployment".into(),
            name: "x".into(),
            namespace: None,
            images: vec![],
            ports: vec![],
            replicas: None,
        };
        assert_eq!(build_k8s_extras(&res), "");
    }

    #[test]
    fn test_build_k8s_extras_full() {
        let res = K8sResource {
            kind: "Deployment".into(),
            name: "x".into(),
            namespace: None,
            images: vec!["app:v1".into()],
            ports: vec![8080, 9090],
            replicas: Some(2),
        };
        let extras = build_k8s_extras(&res);
        assert!(extras.contains("x2"));
        assert!(extras.contains("app:v1"));
        assert!(extras.contains(":8080,9090"));
    }
}
