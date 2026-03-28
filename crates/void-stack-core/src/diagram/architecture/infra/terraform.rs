//! Terraform infrastructure subgraph generation.

use crate::docker;

use super::super::sanitize_id;

/// Generate Terraform subgraph and return `(id, class)` pairs for styling.
pub(crate) fn generate(
    analysis: &docker::DockerAnalysis,
    lines: &mut Vec<String>,
) -> Vec<(String, String)> {
    if analysis.terraform.is_empty() {
        return Vec::new();
    }

    let mut styled = Vec::new();

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
        styled.push((id, class.to_string()));
    }
    lines.push("    end".to_string());

    styled
}
