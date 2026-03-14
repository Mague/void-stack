//! Draw.io (.drawio) XML diagram generation.
//!
//! Generates architecture diagrams in draw.io format that can be opened
//! in diagrams.net, VS Code Draw.io extension, or any compatible editor.
//!
//! Route and DB model scanning is shared with the Mermaid renderer via
//! `api_routes::scan_raw` and `db_models::scan_raw` — no duplication.

use std::path::Path;

use super::service_detection::{self, ServiceType};

use super::api_routes;
use super::db_models;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::security;

const FRONTEND_FILL: &str = "#d5e8d4";
const FRONTEND_STROKE: &str = "#82b366";
const BACKEND_FILL: &str = "#dae8fc";
const BACKEND_STROKE: &str = "#6c8ebf";
const DATABASE_FILL: &str = "#fff2cc";
const DATABASE_STROKE: &str = "#d6b656";
const WORKER_FILL: &str = "#e1d5e7";
const WORKER_STROKE: &str = "#9673a6";
const EXTERNAL_FILL: &str = "#f5f5f5";
const EXTERNAL_STROKE: &str = "#666666";
const CONTAINER_FILL: &str = "#dae8fc";
const CONTAINER_STROKE: &str = "#6c8ebf";

/// Cell ID generator
struct IdGen(u32);

impl IdGen {
    fn new() -> Self {
        IdGen(2) // 0 and 1 are reserved
    }
    fn next(&mut self) -> u32 {
        let id = self.0;
        self.0 += 1;
        id
    }
}

// ServiceType is now in super::service_detection (shared with architecture.rs).

/// Generate a multi-page draw.io file with architecture + API routes + DB models.
pub fn generate_all(project: &Project) -> String {
    let routes = api_routes::scan_raw(project);
    let models = db_models::scan_raw(project);

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<mxfile host=\"void-stack\" agent=\"void-stack\" version=\"1.0\">\n");

    generate_architecture_page(project, &mut xml);
    render_api_routes_page(&routes, &mut xml);
    render_db_models_page(&models, &mut xml);

    xml.push_str("</mxfile>\n");
    xml
}

fn wrap_page(page_xml: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mxfile host=\"void-stack\" agent=\"void-stack\" version=\"1.0\">\n{}</mxfile>\n",
        page_xml
    )
}

/// Generate only the architecture diagram as a standalone Draw.io XML.
pub fn generate_architecture(project: &Project) -> String {
    let mut xml = String::new();
    generate_architecture_page(project, &mut xml);
    wrap_page(&xml)
}

/// Generate only the API routes diagram as a standalone Draw.io XML, if any.
pub fn generate_api_routes(project: &Project) -> Option<String> {
    let routes = api_routes::scan_raw(project);
    if routes.is_empty() {
        return None;
    }
    let mut xml = String::new();
    render_api_routes_page(&routes, &mut xml);
    if xml.contains("mxCell") {
        Some(wrap_page(&xml))
    } else {
        None
    }
}

/// Generate only the DB models diagram as a standalone Draw.io XML, if any.
pub fn generate_db_models(project: &Project) -> Option<String> {
    let models = db_models::scan_raw(project);
    if models.is_empty() {
        return None;
    }
    let mut xml = String::new();
    render_db_models_page(&models, &mut xml);
    if xml.contains("mxCell") {
        Some(wrap_page(&xml))
    } else {
        None
    }
}

// ════════════════════════════════════════════════════════════════════════
// Page 1: Architecture (uses its own lightweight service detection)
// ════════════════════════════════════════════════════════════════════════

fn generate_architecture_page(project: &Project, xml: &mut String) {
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

// ════════════════════════════════════════════════════════════════════════
// Page 2: API Routes (uses shared scanner from api_routes::scan_raw)
// ════════════════════════════════════════════════════════════════════════

fn render_api_routes_page(all_routes: &[(String, Vec<api_routes::Route>)], xml: &mut String) {
    if all_routes.is_empty() {
        return;
    }

    let mut ids = IdGen::new();

    xml.push_str("  <diagram id=\"api\" name=\"API Routes\">\n");
    xml.push_str("    <mxGraphModel dx=\"1422\" dy=\"762\" grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"1\" pageScale=\"1\" pageWidth=\"1600\" pageHeight=\"1200\">\n");
    xml.push_str("      <root>\n");
    xml.push_str("        <mxCell id=\"0\"/>\n");
    xml.push_str("        <mxCell id=\"1\" parent=\"0\"/>\n");

    let mut group_x: u32 = 40;

    for (svc_name, routes) in all_routes {
        let group_id = ids.next();
        let route_h: u32 = 36;
        let route_spacing: u32 = 6;
        let route_w: u32 = 280;
        let header_h: u32 = 30;
        let pad: u32 = 15;
        let group_h = header_h + pad + (routes.len() as u32) * (route_h + route_spacing) + pad;
        let group_w = route_w + 2 * pad;

        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"swimlane;startSize={};fillColor=#dae8fc;strokeColor=#6c8ebf;fontStyle=1;fontSize=13;rounded=1;\" vertex=\"1\" parent=\"1\">\n",
            group_id, esc(svc_name), header_h
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"40\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
            group_x, group_w, group_h
        ));
        xml.push_str("        </mxCell>\n");

        for (i, route) in routes.iter().enumerate() {
            let rid = ids.next();
            let y = header_h + pad + (i as u32) * (route_h + route_spacing);

            let (fill, stroke) = match route.method.as_str() {
                "GET" => ("#d5e8d4", "#82b366"),
                "POST" => ("#fff2cc", "#d6b656"),
                "PUT" => ("#dae8fc", "#6c8ebf"),
                "DELETE" => ("#f8cecc", "#b85450"),
                "PATCH" => ("#e1d5e7", "#9673a6"),
                "WS" => ("#ffe6cc", "#d79b00"),
                "RPC" => ("#e1d5e7", "#9673a6"),
                "STREAM" => ("#e1d5e7", "#9673a6"),
                _ => ("#f5f5f5", "#666666"),
            };

            let label = if let Some(ref summary) = route.summary {
                format!("{} {} — {}", route.method, esc(&route.path), esc(summary))
            } else {
                format!("{} {}", route.method, esc(&route.path))
            };

            xml.push_str(&format!(
                "        <mxCell id=\"{}\" value=\"{}\" style=\"rounded=1;whiteSpace=wrap;html=1;fillColor={};strokeColor={};fontColor=#333333;fontSize=11;align=left;spacingLeft=8;fontStyle=1;\" vertex=\"1\" parent=\"{}\">\n",
                rid, label, fill, stroke, group_id
            ));
            xml.push_str(&format!(
                "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
                pad, y, route_w, route_h
            ));
            xml.push_str("        </mxCell>\n");
        }

        group_x += group_w + 30;
    }

    xml.push_str("      </root>\n");
    xml.push_str("    </mxGraphModel>\n");
    xml.push_str("  </diagram>\n");
}

// ════════════════════════════════════════════════════════════════════════
// Page 3: DB Models (uses shared scanner from db_models::scan_raw)
// ════════════════════════════════════════════════════════════════════════

fn render_db_models_page(all_models: &[db_models::DbModel], xml: &mut String) {
    if all_models.is_empty() {
        return;
    }

    // ── Build adjacency graph for FK relationships ──
    let model_count = all_models.len();
    let model_names: Vec<String> = all_models.iter().map(|m| m.name.clone()).collect();

    let name_to_idx: std::collections::HashMap<String, usize> = model_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_lowercase(), i))
        .collect();

    let mut fk_links: Vec<(usize, usize)> = Vec::new();
    let mut model_fk_targets: Vec<Vec<(String, usize)>> = vec![Vec::new(); model_count];

    for (idx, model) in all_models.iter().enumerate() {
        for (field_name, field_type) in &model.fields {
            let is_fk = field_type == "FK"
                || field_type == "M2M"
                || (field_type == "uuid"
                    && (field_name.ends_with("Id") || field_name.ends_with("_id")));
            if is_fk {
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
                    fk_links.push((idx, tidx));
                    model_fk_targets[idx].push((field_name.clone(), tidx));
                }
            }
        }
    }

    // ── Order models by connectivity (BFS from most-connected) ──
    let mut connection_count: Vec<usize> = vec![0; model_count];
    for (a, b) in &fk_links {
        connection_count[*a] += 1;
        connection_count[*b] += 1;
    }

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); model_count];
    for (a, b) in &fk_links {
        if !adj[*a].contains(b) {
            adj[*a].push(*b);
        }
        if !adj[*b].contains(a) {
            adj[*b].push(*a);
        }
    }

    let mut ordered: Vec<usize> = Vec::with_capacity(model_count);
    let mut visited = vec![false; model_count];

    while ordered.len() < model_count {
        let start = (0..model_count)
            .filter(|i| !visited[*i])
            .max_by_key(|i| connection_count[*i])
            .unwrap();

        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start);
        visited[start] = true;

        while let Some(node) = queue.pop_front() {
            ordered.push(node);
            let mut neighbors: Vec<usize> = adj[node]
                .iter()
                .filter(|n| !visited[**n])
                .copied()
                .collect();
            neighbors.sort_by(|a, b| connection_count[*b].cmp(&connection_count[*a]));
            for n in neighbors {
                if !visited[n] {
                    visited[n] = true;
                    queue.push_back(n);
                }
            }
        }
    }

    // ── Layout ──
    let mut ids = IdGen::new();
    let cols = 4.min(model_count).max(1);
    let card_w: u32 = 240;
    let row_h: u32 = 22;
    let header_h: u32 = 30;
    let pad: u32 = 10;
    let spacing_x: u32 = 60;
    let spacing_y: u32 = 50;

    let card_heights: Vec<u32> = all_models
        .iter()
        .map(|m| header_h + pad + (m.fields.len() as u32) * row_h + pad)
        .collect();

    let num_rows = model_count.div_ceil(cols);
    let mut row_max_h: Vec<u32> = vec![0; num_rows];
    for (pos, &model_idx) in ordered.iter().enumerate() {
        let row = pos / cols;
        row_max_h[row] = row_max_h[row].max(card_heights[model_idx]);
    }

    let mut row_y: Vec<u32> = vec![40; num_rows];
    for r in 1..num_rows {
        row_y[r] = row_y[r - 1] + row_max_h[r - 1] + spacing_y;
    }

    let total_w = 40 + (cols as u32) * (card_w + spacing_x);
    let total_h = if num_rows > 0 {
        row_y[num_rows - 1] + row_max_h[num_rows - 1] + 80
    } else {
        800
    };

    xml.push_str("  <diagram id=\"db\" name=\"DB Models\">\n");
    xml.push_str(&format!(
        "    <mxGraphModel dx=\"1422\" dy=\"762\" grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"1\" pageScale=\"1\" pageWidth=\"{}\" pageHeight=\"{}\">\n",
        total_w.max(2400), total_h.max(1600)
    ));
    xml.push_str("      <root>\n");
    xml.push_str("        <mxCell id=\"0\"/>\n");
    xml.push_str("        <mxCell id=\"1\" parent=\"0\"/>\n");

    let mut model_cell_ids: Vec<u32> = vec![0; model_count];

    for (pos, &model_idx) in ordered.iter().enumerate() {
        let model = &all_models[model_idx];
        let col = pos % cols;
        let row = pos / cols;
        let card_h = card_heights[model_idx];
        let x = 40 + (col as u32) * (card_w + spacing_x);
        let y = row_y[row];

        let group_id = ids.next();
        model_cell_ids[model_idx] = group_id;

        // Table container
        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"swimlane;startSize={};fillColor=#dae8fc;strokeColor=#6c8ebf;fontStyle=1;fontSize=13;rounded=1;collapsible=0;\" vertex=\"1\" parent=\"1\">\n",
            group_id, esc(&model.name), header_h
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
            x, y, card_w, card_h
        ));
        xml.push_str("        </mxCell>\n");

        for (fi, (field_name, field_type)) in model.fields.iter().enumerate() {
            let fid = ids.next();
            let fy = header_h + pad + (fi as u32) * row_h;

            let is_fk = field_type == "FK"
                || field_type == "M2M"
                || (field_type == "uuid"
                    && (field_name.ends_with("Id") || field_name.ends_with("_id")));
            let icon = if field_type == "FK" || field_type == "M2M" {
                "🔗 "
            } else if field_name == "id" {
                "🔑 "
            } else {
                ""
            };

            let label = format!("{}{}: {}", icon, esc(field_name), field_type);

            let (fill, stroke) = if field_name == "id" {
                ("#fff2cc", "#d6b656")
            } else if is_fk {
                ("#f8cecc", "#b85450")
            } else {
                ("#ffffff", "#d6d6d6")
            };

            xml.push_str(&format!(
                "        <mxCell id=\"{}\" value=\"{}\" style=\"text;html=1;align=left;verticalAlign=middle;resizable=0;points=[];autosize=1;fillColor={};strokeColor={};rounded=1;spacingLeft=4;fontSize=11;\" vertex=\"1\" parent=\"{}\">\n",
                fid, label, fill, stroke, group_id
            ));
            xml.push_str(&format!(
                "          <mxGeometry x=\"4\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
                fy, card_w - 8, row_h
            ));
            xml.push_str("        </mxCell>\n");
        }
    }

    // ── FK edges ──
    for (source_idx, targets) in model_fk_targets.iter().enumerate() {
        let source_id = model_cell_ids[source_idx];
        for (_field_name, target_idx) in targets {
            let target_id = model_cell_ids[*target_idx];
            if source_id == 0 || target_id == 0 || source_id == target_id {
                continue;
            }

            let eid = ids.next();
            xml.push_str(&format!(
                "        <mxCell id=\"{}\" style=\"endArrow=ERmandOne;startArrow=ERzeroToMany;html=1;strokeWidth=1;strokeColor=#666666;curved=1;\" edge=\"1\" source=\"{}\" target=\"{}\" parent=\"1\">\n",
                eid, source_id, target_id
            ));
            xml.push_str("          <mxGeometry relative=\"1\" as=\"geometry\"/>\n");
            xml.push_str("        </mxCell>\n");
        }
    }

    xml.push_str("      </root>\n");
    xml.push_str("    </mxGraphModel>\n");
    xml.push_str("  </diagram>\n");
}

// ════════════════════════════════════════════════════════════════════════
// Architecture helpers (lightweight service/external detection for layout)
// detect_service_info and extract_port are now in super::service_detection
// ════════════════════════════════════════════════════════════════════════

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

/// XML-escape a string for use in attribute values.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, Service, Target};

    fn make_service(name: &str, command: &str, dir: &std::path::Path) -> Service {
        Service {
            name: name.to_string(),
            command: command.to_string(),
            target: Target::Windows,
            working_dir: Some(dir.to_string_lossy().to_string()),
            enabled: true,
            env_vars: Vec::new(),
            depends_on: Vec::new(),
            docker: None,
        }
    }

    fn make_project(dir: &std::path::Path) -> Project {
        Project {
            name: "test-project".to_string(),
            description: String::new(),
            path: dir.to_string_lossy().to_string(),
            project_type: None,
            tags: Vec::new(),
            services: vec![make_service("api", "npm start", dir)],
            hooks: None,
        }
    }

    #[test]
    fn test_esc() {
        assert_eq!(esc("hello"), "hello");
        assert_eq!(esc("<b>bold</b>"), "&lt;b&gt;bold&lt;/b&gt;");
        assert_eq!(esc("a & b"), "a &amp; b");
        assert_eq!(esc(r#"say "hi""#), "say &quot;hi&quot;");
    }

    #[test]
    fn test_id_gen() {
        let mut id_gen = IdGen::new();
        assert_eq!(id_gen.next(), 2);
        assert_eq!(id_gen.next(), 3);
        assert_eq!(id_gen.next(), 4);
    }

    #[test]
    fn test_generate_all_structure() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());

        let xml = generate_all(&project);
        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("<mxfile"));
        assert!(xml.contains("</mxfile>"));
        assert!(xml.contains("Architecture"));
        assert!(xml.contains("test-project"));
    }

    #[test]
    fn test_generate_architecture() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());

        let xml = generate_architecture(&project);
        assert!(xml.contains("mxGraphModel"));
        assert!(xml.contains("api"));
    }

    #[test]
    fn test_generate_api_routes_none() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());

        let result = generate_api_routes(&project);
        assert!(result.is_none());
    }

    #[test]
    fn test_generate_api_routes_with_routes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            r#"
from fastapi import FastAPI
app = FastAPI()

@app.get("/users")
def list_users():
    pass

@app.post("/users")
def create_user():
    pass
"#,
        )
        .unwrap();

        let project = make_project(dir.path());
        let result = generate_api_routes(&project);
        assert!(result.is_some());
        let xml = result.unwrap();
        assert!(xml.contains("/users"));
        assert!(xml.contains("GET"));
        assert!(xml.contains("POST"));
    }

    #[test]
    fn test_generate_db_models_none() {
        let dir = tempfile::tempdir().unwrap();
        let project = make_project(dir.path());

        let result = generate_db_models(&project);
        assert!(result.is_none());
    }

    #[test]
    fn test_generate_db_models_with_models() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("prisma")).unwrap();
        std::fs::write(
            dir.path().join("prisma/schema.prisma"),
            r#"
model User {
  id    Int    @id
  name  String
  email String
}
"#,
        )
        .unwrap();

        let project = make_project(dir.path());
        let result = generate_db_models(&project);
        assert!(result.is_some());
        let xml = result.unwrap();
        assert!(xml.contains("User"));
    }

    #[test]
    fn test_add_if() {
        let mut list = Vec::new();
        add_if(&mut list, "image: postgres:16", "postgres", "PostgreSQL");
        add_if(&mut list, "image: postgres:16", "postgres", "PostgreSQL"); // no dup
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], "PostgreSQL");
    }

    #[test]
    fn test_detect_external_services_from_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "REDIS_URL=redis://localhost\n").unwrap();

        let project = make_project(dir.path());
        let externals = detect_external_services(dir.path(), &project);
        assert!(externals.iter().any(|e| e == "Redis"));
    }

    #[test]
    fn test_generate_all_multi_service() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: "multi".to_string(),
            description: String::new(),
            path: dir.path().to_string_lossy().to_string(),
            project_type: None,
            tags: Vec::new(),
            services: vec![
                make_service("frontend", "npm start", dir.path()),
                make_service("backend", "python main.py", dir.path()),
            ],
            hooks: None,
        };

        let xml = generate_all(&project);
        assert!(xml.contains("frontend"));
        assert!(xml.contains("backend"));
    }
}
