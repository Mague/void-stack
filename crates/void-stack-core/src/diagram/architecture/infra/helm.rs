//! Helm chart subgraph generation.

use crate::docker;

use super::super::sanitize_id;

/// Generate Helm chart subgraph and return dependency node IDs for styling.
pub(crate) fn generate(analysis: &docker::DockerAnalysis, lines: &mut Vec<String>) -> Vec<String> {
    let chart = match &analysis.helm {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut helm_ids = Vec::new();

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
        helm_ids.push(id);
    }
    lines.push("    end".to_string());

    helm_ids
}
