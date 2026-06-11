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
