//! Draw.io architecture page generation (renders the shared `DiagramIr`).

use std::collections::HashMap;

use super::super::ir::{ArchEdgeKind, DiagramIr, sanitize_id};
use super::super::service_detection::ServiceType;

use super::common::*;

pub(crate) fn generate_architecture_page(ir: &DiagramIr, xml: &mut String) {
    let mut ids = IdGen::new();
    // Logical node name → draw.io cell id (services, externals, crates, infra).
    let mut cell_of: HashMap<String, u32> = HashMap::new();

    // Layout
    let svc_count = ir.services.len().max(1);
    let node_w: u32 = 160;
    let node_h: u32 = 60;
    let spacing: u32 = 50;
    let cols = if svc_count <= 3 { svc_count } else { 3 };
    let rows = svc_count.div_ceil(cols);
    let container_id = ids.next();
    let header: u32 = 40;
    let pad: u32 = 30;
    let cw = (cols as u32) * node_w + (cols as u32 + 1) * spacing;
    let ch = header + pad + (rows as u32) * node_h + (rows as u32) * spacing;
    let cx: u32 = 60;
    let cy: u32 = 40;

    xml.push_str("  <diagram id=\"arch\" name=\"Architecture\">\n");
    xml.push_str("    <mxGraphModel dx=\"1422\" dy=\"762\" grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"1\" pageScale=\"1\" pageWidth=\"1169\" pageHeight=\"827\">\n");
    xml.push_str("      <root>\n");
    xml.push_str("        <mxCell id=\"0\"/>\n");
    xml.push_str("        <mxCell id=\"1\" parent=\"0\"/>\n");

    // Container group
    xml.push_str(&format!(
        "        <mxCell id=\"{}\" value=\"{}\" style=\"rounded=1;whiteSpace=wrap;html=1;container=1;collapsible=0;fillColor={};strokeColor={};fontStyle=1;verticalAlign=top;fontSize=14;strokeWidth=2;opacity=30;\" vertex=\"1\" parent=\"1\">\n",
        container_id, esc(&ir.project_name), CONTAINER_FILL, CONTAINER_STROKE
    ));
    xml.push_str(&format!(
        "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
        cx, cy, cw, ch
    ));
    xml.push_str("        </mxCell>\n");

    // Service nodes
    for (i, svc) in ir.services.iter().enumerate() {
        let nid = ids.next();
        cell_of.insert(svc.name.clone(), nid);
        let col = i % cols;
        let row = i / cols;
        let x = spacing + (col as u32) * (node_w + spacing);
        let y = header + pad + (row as u32) * (node_h + spacing);

        let (fill, stroke, shape) = match svc.service_type {
            ServiceType::Frontend => (FRONTEND_FILL, FRONTEND_STROKE, "rounded=1;"),
            ServiceType::Backend => (BACKEND_FILL, BACKEND_STROKE, "rounded=1;"),
            ServiceType::Database => (
                DATABASE_FILL,
                DATABASE_STROKE,
                "shape=cylinder3;boundedLbl=1;backgroundOutline=1;size=12;",
            ),
            ServiceType::Worker => (WORKER_FILL, WORKER_STROKE, "rounded=1;"),
            ServiceType::Unknown => (EXTERNAL_FILL, EXTERNAL_STROKE, "rounded=1;"),
        };

        let type_label = match svc.service_type {
            ServiceType::Frontend => "Frontend",
            ServiceType::Backend => "API",
            ServiceType::Database => "Database",
            ServiceType::Worker => "Worker",
            ServiceType::Unknown => svc.command.as_str(),
        };
        let port_str = svc.port.map(|p| format!(" :{}", p)).unwrap_or_default();
        let label = format!("{}{}\n{}", esc(&svc.name), port_str, type_label);

        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"{}whiteSpace=wrap;html=1;fillColor={};strokeColor={};fontColor=#333333;fontSize=12;fontStyle=1;\" vertex=\"1\" parent=\"{}\">\n",
            nid, label, shape, fill, stroke, container_id
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
            x, y, node_w, node_h
        ));
        xml.push_str("        </mxCell>\n");
    }

    // External nodes (right column)
    let ext_x = cx + cw + 80;
    for (i, name) in ir.externals.iter().enumerate() {
        let eid = ids.next();
        cell_of.insert(name.clone(), eid);
        let ey = cy + 20 + (i as u32) * 90;
        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"rounded=1;whiteSpace=wrap;html=1;fillColor={};strokeColor={};fontColor=#333333;fontSize=12;fontStyle=1;dashed=1;dashPattern=8 4;\" vertex=\"1\" parent=\"1\">\n",
            eid, esc(name), EXTERNAL_FILL, EXTERNAL_STROKE
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"140\" height=\"50\" as=\"geometry\"/>\n",
            ext_x, ey
        ));
        xml.push_str("        </mxCell>\n");
    }

    let mut next_band_y = cy + ch + 60;

    // Rust crates group (below the service container)
    if !ir.crate_links.is_empty() {
        let mut crate_names: Vec<&str> = Vec::new();
        for (from, to) in &ir.crate_links {
            if !crate_names.contains(&from.as_str()) {
                crate_names.push(from);
            }
            if !crate_names.contains(&to.as_str()) {
                crate_names.push(to);
            }
        }
        next_band_y = render_group(
            xml,
            &mut ids,
            &mut cell_of,
            "Rust Crates",
            &crate_names
                .iter()
                .map(|n| (format!("crate_{}", sanitize_id(n)), format!("📦 {}", n)))
                .collect::<Vec<_>>(),
            ("#ffe6cc", "#d79b00"),
            cx,
            next_band_y,
        );
        for (from, to) in &ir.crate_links {
            let (Some(&fid), Some(&tid)) = (
                cell_of.get(&format!("crate_{}", sanitize_id(from))),
                cell_of.get(&format!("crate_{}", sanitize_id(to))),
            ) else {
                continue;
            };
            push_edge(xml, &mut ids, fid, tid, Some("dep"), false);
        }
    }

    // Infrastructure groups (Terraform / Kubernetes / Helm)
    let tf_nodes: Vec<(String, String)> = ir
        .infra
        .terraform
        .iter()
        .map(|res| {
            let details = if res.details.is_empty() {
                String::new()
            } else {
                format!("\n{}", res.details.join(", "))
            };
            (
                format!(
                    "tf_{}_{}",
                    sanitize_id(&res.provider),
                    sanitize_id(&res.name)
                ),
                format!("{} {}{}", res.resource_type, res.name, details),
            )
        })
        .collect();
    if !tf_nodes.is_empty() {
        next_band_y = render_group(
            xml,
            &mut ids,
            &mut cell_of,
            "Infrastructure (Terraform)",
            &tf_nodes,
            ("#f8cecc", "#b85450"),
            cx,
            next_band_y,
        );
    }

    let k8s_nodes: Vec<(String, String)> = ir
        .infra
        .kubernetes
        .iter()
        .map(|res| {
            (
                format!("k8s_{}_{}", sanitize_id(&res.kind), sanitize_id(&res.name)),
                format!("{}: {}", res.kind, res.name),
            )
        })
        .collect();
    if !k8s_nodes.is_empty() {
        next_band_y = render_group(
            xml,
            &mut ids,
            &mut cell_of,
            "Kubernetes",
            &k8s_nodes,
            ("#dae8fc", "#326CE5"),
            cx,
            next_band_y,
        );
    }

    if let Some(helm) = &ir.infra.helm {
        let helm_nodes: Vec<(String, String)> = helm
            .dependencies
            .iter()
            .map(|dep| {
                (
                    format!("helm_{}", sanitize_id(&dep.name)),
                    format!("{} ({})", dep.name, dep.version),
                )
            })
            .collect();
        if !helm_nodes.is_empty() {
            render_group(
                xml,
                &mut ids,
                &mut cell_of,
                &format!("Helm: {} v{}", helm.name, helm.version),
                &helm_nodes,
                ("#e1d5e7", "#0F1689"),
                cx,
                next_band_y,
            );
        }
    }

    // Edges from the IR
    for edge in &ir.edges {
        let Some(&from_id) = cell_of.get(&edge.from) else {
            continue;
        };
        let Some(&to_id) = cell_of.get(&edge.to) else {
            continue;
        };
        let dashed = matches!(edge.kind, ArchEdgeKind::External | ArchEdgeKind::Infra);
        push_edge(xml, &mut ids, from_id, to_id, edge.label.as_deref(), dashed);
    }

    xml.push_str("      </root>\n");
    xml.push_str("    </mxGraphModel>\n");
    xml.push_str("  </diagram>\n");
}

/// Render a titled swimlane with a grid of nodes. Registers each node's
/// cell id under its logical key in `cell_of`. Returns the y below the group.
#[allow(clippy::too_many_arguments)]
fn render_group(
    xml: &mut String,
    ids: &mut IdGen,
    cell_of: &mut HashMap<String, u32>,
    title: &str,
    nodes: &[(String, String)], // (logical key, label)
    colors: (&str, &str),
    x: u32,
    y: u32,
) -> u32 {
    let node_w: u32 = 180;
    let node_h: u32 = 50;
    let spacing: u32 = 20;
    let header: u32 = 30;
    let cols = nodes.len().clamp(1, 4);
    let rows = nodes.len().div_ceil(cols);
    let gw = (cols as u32) * (node_w + spacing) + spacing;
    let gh = header + (rows as u32) * (node_h + spacing) + spacing;

    let gid = ids.next();
    xml.push_str(&format!(
        "        <mxCell id=\"{}\" value=\"{}\" style=\"swimlane;startSize={};fillColor=none;strokeColor={};fontStyle=1;fontSize=13;rounded=1;collapsible=0;\" vertex=\"1\" parent=\"1\">\n",
        gid, esc(title), header, colors.1
    ));
    xml.push_str(&format!(
        "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
        x, y, gw, gh
    ));
    xml.push_str("        </mxCell>\n");

    for (i, (key, label)) in nodes.iter().enumerate() {
        let nid = ids.next();
        cell_of.insert(key.clone(), nid);
        let col = i % cols;
        let row = i / cols;
        let nx = spacing + (col as u32) * (node_w + spacing);
        let ny = header + spacing + (row as u32) * (node_h + spacing);
        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"rounded=1;whiteSpace=wrap;html=1;fillColor={};strokeColor={};fontColor=#333333;fontSize=11;\" vertex=\"1\" parent=\"{}\">\n",
            nid,
            esc(label),
            colors.0,
            colors.1,
            gid
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
            nx, ny, node_w, node_h
        ));
        xml.push_str("        </mxCell>\n");
    }

    y + gh + 40
}

fn push_edge(
    xml: &mut String,
    ids: &mut IdGen,
    source: u32,
    target: u32,
    label: Option<&str>,
    dashed: bool,
) {
    let eid = ids.next();
    let value = label
        .map(|l| format!(" value=\"{}\"", esc(l)))
        .unwrap_or_default();
    let style = if dashed {
        "endArrow=classic;html=1;strokeWidth=1;strokeColor=#999999;dashed=1;"
    } else {
        "endArrow=classic;html=1;strokeWidth=2;strokeColor=#333333;"
    };
    xml.push_str(&format!(
        "        <mxCell id=\"{}\"{} style=\"{}\" edge=\"1\" source=\"{}\" target=\"{}\" parent=\"1\">\n",
        eid, value, style, source, target
    ));
    xml.push_str("          <mxGeometry relative=\"1\" as=\"geometry\"/>\n");
    xml.push_str("        </mxCell>\n");
}

#[cfg(test)]
mod tests {
    use super::super::super::ir::{ArchEdge, ServiceNode};
    use super::*;
    use crate::docker::{
        DockerAnalysis, HelmChart, HelmDependency, InfraResource, InfraResourceKind, K8sResource,
    };

    /// Build a minimal service node for the architecture IR.
    fn svc(name: &str, service_type: ServiceType, port: Option<u16>) -> ServiceNode {
        ServiceNode {
            name: name.to_string(),
            service_type,
            port,
            command: format!("run-{}", name),
        }
    }

    /// Build an IR with the given project name and services; everything else empty.
    fn base_ir(project_name: &str, services: Vec<ServiceNode>) -> DiagramIr {
        DiagramIr {
            project_name: project_name.to_string(),
            services,
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

    fn render(ir: &DiagramIr) -> String {
        let mut xml = String::new();
        generate_architecture_page(ir, &mut xml);
        xml
    }

    /// Return the first XML line containing `needle` (for style assertions).
    fn line_with<'a>(xml: &'a str, needle: &str) -> &'a str {
        xml.lines()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("expected a line containing {:?} in:\n{}", needle, xml))
    }

    #[test]
    fn test_generates_diagram_skeleton_with_project_container() {
        let ir = base_ir("my-project", vec![svc("web", ServiceType::Frontend, None)]);
        let xml = render(&ir);

        assert!(
            xml.contains("<diagram id=\"arch\" name=\"Architecture\">"),
            "should open the Architecture diagram page"
        );
        assert!(xml.contains("</diagram>"), "diagram element must be closed");
        assert!(
            xml.contains("<mxCell id=\"0\"/>") && xml.contains("<mxCell id=\"1\" parent=\"0\"/>"),
            "root cells 0 and 1 are required by draw.io"
        );
        let container = line_with(&xml, "value=\"my-project\"");
        assert!(
            container.contains("container=1"),
            "project cell should be a container group: {}",
            container
        );
    }

    #[test]
    fn test_service_nodes_have_type_labels_ports_and_shapes() {
        let ir = base_ir(
            "demo",
            vec![
                svc("web", ServiceType::Frontend, Some(3000)),
                svc("api", ServiceType::Backend, Some(8080)),
                svc("db", ServiceType::Database, None),
            ],
        );
        let xml = render(&ir);

        assert!(
            xml.contains("value=\"web :3000\nFrontend\""),
            "frontend label should include port and type: {}",
            xml
        );
        assert!(
            xml.contains("value=\"api :8080\nAPI\""),
            "backend label should render as API"
        );
        assert!(
            xml.contains("value=\"db\nDatabase\""),
            "database without port should omit the port suffix"
        );
        // Labels embed a literal newline, so the style attribute lives on the
        // physical line that starts with the type label.
        let db_line = line_with(&xml, "Database\" style=");
        assert!(
            db_line.contains("shape=cylinder3"),
            "database nodes should be cylinders: {}",
            db_line
        );
        let web_line = line_with(&xml, "Frontend\" style=");
        assert!(
            web_line.contains(FRONTEND_FILL) && web_line.contains(FRONTEND_STROKE),
            "frontend node should use frontend colors: {}",
            web_line
        );
    }

    #[test]
    fn test_escapes_special_chars_in_names() {
        let mut ir = base_ir("A & B", vec![svc("cache<1>", ServiceType::Worker, None)]);
        ir.externals.push("S3 & Friends".to_string());
        let xml = render(&ir);

        assert!(
            xml.contains("value=\"A &amp; B\""),
            "project name ampersand must be escaped"
        );
        assert!(
            xml.contains("cache&lt;1&gt;"),
            "angle brackets in service names must be escaped"
        );
        assert!(
            xml.contains("value=\"S3 &amp; Friends\""),
            "external names must be escaped"
        );
        assert!(
            !xml.contains("value=\"cache<1>"),
            "raw unescaped service name must not appear in attribute values"
        );
    }

    #[test]
    fn test_external_nodes_render_dashed() {
        let mut ir = base_ir("demo", vec![svc("api", ServiceType::Backend, None)]);
        ir.externals.push("Stripe".to_string());
        let xml = render(&ir);

        let ext_line = line_with(&xml, "value=\"Stripe\"");
        assert!(
            ext_line.contains("dashed=1") && ext_line.contains("dashPattern"),
            "external nodes should have dashed borders: {}",
            ext_line
        );
    }

    #[test]
    fn test_edges_use_labels_and_dash_style_by_kind() {
        let mut ir = base_ir(
            "demo",
            vec![
                svc("web", ServiceType::Frontend, None),
                svc("api", ServiceType::Backend, None),
            ],
        );
        ir.externals.push("Stripe".to_string());
        ir.edges = vec![
            ArchEdge {
                from: "web".to_string(),
                to: "api".to_string(),
                label: Some("REST".to_string()),
                kind: ArchEdgeKind::Api,
            },
            ArchEdge {
                from: "api".to_string(),
                to: "Stripe".to_string(),
                label: None,
                kind: ArchEdgeKind::External,
            },
            // Unknown endpoint: must be silently skipped, not panic.
            ArchEdge {
                from: "web".to_string(),
                to: "ghost".to_string(),
                label: None,
                kind: ArchEdgeKind::Api,
            },
        ];
        let xml = render(&ir);

        let api_edge = line_with(&xml, "value=\"REST\"");
        assert!(
            api_edge.contains("edge=\"1\"") && !api_edge.contains("dashed=1"),
            "service-to-service edges should be solid: {}",
            api_edge
        );
        assert_eq!(
            xml.matches("edge=\"1\"").count(),
            2,
            "edge to unknown node must be skipped"
        );
        assert_eq!(
            xml.matches("dashed=1;\" edge=\"1\"").count(),
            1,
            "external edge should be dashed"
        );
    }

    #[test]
    fn test_crate_links_render_group_and_dep_edges() {
        let mut ir = base_ir("demo", vec![svc("api", ServiceType::Backend, None)]);
        ir.crate_links = vec![("core".to_string(), "proto".to_string())];
        let xml = render(&ir);

        assert!(
            xml.contains("value=\"Rust Crates\""),
            "crate swimlane should be titled Rust Crates"
        );
        assert!(
            xml.contains("value=\"📦 core\"") && xml.contains("value=\"📦 proto\""),
            "each crate should get a package node"
        );
        assert!(
            xml.contains("value=\"dep\""),
            "crate dependency edges should be labeled dep"
        );
    }

    #[test]
    fn test_infra_groups_render_terraform_kubernetes_and_helm() {
        let mut ir = base_ir("demo", vec![svc("api", ServiceType::Backend, None)]);
        ir.infra.terraform.push(InfraResource {
            provider: "aws".to_string(),
            resource_type: "aws_db_instance".to_string(),
            name: "main".to_string(),
            kind: InfraResourceKind::Database,
            details: vec!["postgres".to_string(), "15".to_string()],
        });
        ir.infra.kubernetes.push(K8sResource {
            kind: "Deployment".to_string(),
            name: "web".to_string(),
            namespace: None,
            images: Vec::new(),
            ports: Vec::new(),
            replicas: Some(2),
        });
        ir.infra.helm = Some(HelmChart {
            name: "mychart".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec![HelmDependency {
                name: "redis".to_string(),
                version: "17.0.0".to_string(),
                repository: "https://charts.bitnami.com".to_string(),
            }],
        });
        let xml = render(&ir);

        assert!(
            xml.contains("value=\"Infrastructure (Terraform)\""),
            "terraform swimlane should be rendered"
        );
        assert!(
            xml.contains("aws_db_instance main\npostgres, 15"),
            "terraform node label should include type, name and details: {}",
            xml
        );
        assert!(
            xml.contains("value=\"Kubernetes\"") && xml.contains("value=\"Deployment: web\""),
            "kubernetes swimlane and node should be rendered"
        );
        assert!(
            xml.contains("value=\"Helm: mychart v1.0.0\""),
            "helm swimlane title should include chart name and version"
        );
        assert!(
            xml.contains("value=\"redis (17.0.0)\""),
            "helm dependency should be rendered with its version"
        );
    }
}
