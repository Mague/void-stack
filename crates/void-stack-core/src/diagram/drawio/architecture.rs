//! Draw.io architecture page generation.

use std::path::Path;

use crate::diagram::service_detection::{self, ServiceType};
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::security;

use super::common::*;

pub(crate) fn generate_architecture_page(project: &Project, xml: &mut String) {
    let mut ids = IdGen::new();

    // Detect services
    let mut svc_nodes: Vec<(u32, String, ServiceType, Option<u16>, String)> = Vec::new();
    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);
        let (svc_type, port) = service_detection::detect_service_info(dir_path, &svc.command);
        let node_id = ids.next();
        svc_nodes.push((
            node_id,
            svc.name.clone(),
            svc_type,
            port,
            svc.command.clone(),
        ));
    }

    // Detect external services
    let root = strip_win_prefix(&project.path);
    let root_path = Path::new(&root);
    let external_names = detect_external_services(root_path, project);
    let mut ext_nodes: Vec<(u32, String)> = Vec::new();
    for name in &external_names {
        ext_nodes.push((ids.next(), name.clone()));
    }

    // Layout
    let svc_count = svc_nodes.len().max(1);
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
        container_id, esc(&project.name), CONTAINER_FILL, CONTAINER_STROKE
    ));
    xml.push_str(&format!(
        "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
        cx, cy, cw, ch
    ));
    xml.push_str("        </mxCell>\n");

    // Service nodes
    for (i, (nid, name, svc_type, port, cmd)) in svc_nodes.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let x = spacing + (col as u32) * (node_w + spacing);
        let y = header + pad + (row as u32) * (node_h + spacing);

        let (fill, stroke, shape) = match svc_type {
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

        let type_label = match svc_type {
            ServiceType::Frontend => "Frontend",
            ServiceType::Backend => "API",
            ServiceType::Database => "Database",
            ServiceType::Worker => "Worker",
            ServiceType::Unknown => cmd.as_str(),
        };
        let port_str = port.map(|p| format!(" :{}", p)).unwrap_or_default();
        let label = format!("{}{}\n{}", esc(name), port_str, type_label);

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

    // External nodes
    let ext_x = cx + cw + 80;
    for (i, (eid, name)) in ext_nodes.iter().enumerate() {
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

    // Edges: frontend → backend
    for (fid, _, ft, _, _) in &svc_nodes {
        if matches!(ft, ServiceType::Frontend) {
            for (bid, _, bt, _, _) in &svc_nodes {
                if matches!(bt, ServiceType::Backend) {
                    let eid = ids.next();
                    xml.push_str(&format!(
                        "        <mxCell id=\"{}\" value=\"API\" style=\"endArrow=classic;html=1;strokeWidth=2;strokeColor=#333333;\" edge=\"1\" source=\"{}\" target=\"{}\" parent=\"{}\">\n",
                        eid, fid, bid, container_id
                    ));
                    xml.push_str("          <mxGeometry relative=\"1\" as=\"geometry\"/>\n");
                    xml.push_str("        </mxCell>\n");
                }
            }
        }
    }

    // Edges: backend → external
    for (bid, _, bt, _, _) in &svc_nodes {
        if matches!(bt, ServiceType::Backend) {
            for (eid, _) in &ext_nodes {
                let edge_id = ids.next();
                xml.push_str(&format!(
                    "        <mxCell id=\"{}\" style=\"endArrow=classic;html=1;strokeWidth=1;strokeColor=#999999;dashed=1;\" edge=\"1\" source=\"{}\" target=\"{}\" parent=\"1\">\n",
                    edge_id, bid, eid
                ));
                xml.push_str("          <mxGeometry relative=\"1\" as=\"geometry\"/>\n");
                xml.push_str("        </mxCell>\n");
            }
        }
    }

    xml.push_str("      </root>\n");
    xml.push_str("    </mxGraphModel>\n");
    xml.push_str("  </diagram>\n");
}

fn detect_external_services(root: &Path, project: &Project) -> Vec<String> {
    let mut externals = Vec::new();
    let mut dirs_to_scan: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    for svc in &project.services {
        if let Some(dir) = &svc.working_dir {
            dirs_to_scan.push(Path::new(&strip_win_prefix(dir)).to_path_buf());
        }
    }

    for dir in &dirs_to_scan {
        for env_file in &[".env", ".env.example"] {
            let env_path = dir.join(env_file);
            let keys = security::read_env_keys(&env_path);
            let keys_upper: String = keys.join(" ").to_uppercase();

            add_if(&mut externals, &keys_upper, "POSTGRES", "PostgreSQL");
            add_if(&mut externals, &keys_upper, "DATABASE_URL", "PostgreSQL");
            add_if(&mut externals, &keys_upper, "REDIS", "Redis");
            add_if(&mut externals, &keys_upper, "MONGO", "MongoDB");
            add_if(&mut externals, &keys_upper, "OLLAMA", "Ollama");
            add_if(&mut externals, &keys_upper, "OPENAI", "AI API");
            add_if(&mut externals, &keys_upper, "ANTHROPIC", "AI API");
            add_if(&mut externals, &keys_upper, "S3", "AWS S3");
        }
        for compose in &["docker-compose.yml", "docker-compose.yaml", "compose.yml"] {
            if let Ok(content) = std::fs::read_to_string(dir.join(compose)) {
                let lower = content.to_lowercase();
                add_if(&mut externals, &lower, "postgres", "PostgreSQL");
                add_if(&mut externals, &lower, "redis", "Redis");
                add_if(&mut externals, &lower, "mongo", "MongoDB");
                add_if(&mut externals, &lower, "rabbitmq", "RabbitMQ");
            }
        }
    }
    externals
}

fn add_if(list: &mut Vec<String>, haystack: &str, keyword: &str, service: &str) {
    if haystack.contains(keyword) && !list.contains(&service.to_string()) {
        list.push(service.to_string());
    }
}

// Test helpers exposed for mod.rs tests
#[cfg(test)]
pub(super) fn tests_helper_add_if(
    list: &mut Vec<String>,
    haystack: &str,
    keyword: &str,
    service: &str,
) {
    add_if(list, haystack, keyword, service);
}

#[cfg(test)]
pub(super) fn tests_helper_detect_externals(root: &Path, project: &Project) -> Vec<String> {
    detect_external_services(root, project)
}
