//! Infrastructure diagram generation (Terraform, Kubernetes, Helm).

mod helm;
pub(crate) mod kubernetes;
mod terraform;

use crate::docker;

/// Generate Mermaid subgraphs for Terraform, Kubernetes, and Helm resources.
/// Returns a list of node IDs for infrastructure resources (used for connections).
pub(super) fn generate_infra_subgraphs(
    analysis: &docker::DockerAnalysis,
    lines: &mut Vec<String>,
) -> Vec<String> {
    let tf_styled = terraform::generate(analysis, lines);
    let k8s_ids = kubernetes::generate(analysis, lines);
    let helm_ids = helm::generate(analysis, lines);

    // Apply styling classes
    let mut result_ids = Vec::new();
    for (id, class) in &tf_styled {
        lines.push(format!("    class {} {}", id, class));
        result_ids.push(id.clone());
    }
    for id in &k8s_ids {
        lines.push(format!("    class {} k8s", id));
    }
    for id in &helm_ids {
        lines.push(format!("    class {} helm", id));
    }

    result_ids
}

#[cfg(test)]
mod tests {
    use super::generate_infra_subgraphs;
    use super::kubernetes as k8s_mod;
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
        assert_eq!(k8s_mod::build_extras(&res), "");
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
        let extras = k8s_mod::build_extras(&res);
        assert!(extras.contains("x2"));
        assert!(extras.contains("app:v1"));
        assert!(extras.contains(":8080,9090"));
    }
}
