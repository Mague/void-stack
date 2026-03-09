//! Draw.io (.drawio) XML diagram generation.
//!
//! Generates architecture diagrams in draw.io format that can be opened
//! in diagrams.net, VS Code Draw.io extension, or any compatible editor.

use std::path::Path;

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

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum ServiceType {
    Frontend,
    Backend,
    Database,
    Worker,
    Unknown,
}

/// Generate a multi-page draw.io file with architecture + API routes + DB models.
pub fn generate_all(project: &Project) -> String {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<mxfile host=\"void-stack\" agent=\"void-stack\" version=\"1.0\">\n");

    // Page 1: Architecture
    generate_architecture_page(project, &mut xml);

    // Page 2: API Routes (if any)
    generate_api_routes_page(project, &mut xml);

    // Page 3: DB Models (if any)
    generate_db_models_page(project, &mut xml);

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
    let mut xml = String::new();
    generate_api_routes_page(project, &mut xml);
    if xml.contains("mxCell") {
        Some(wrap_page(&xml))
    } else {
        None
    }
}

/// Generate only the DB models diagram as a standalone Draw.io XML, if any.
pub fn generate_db_models(project: &Project) -> Option<String> {
    let mut xml = String::new();
    generate_db_models_page(project, &mut xml);
    if xml.contains("mxCell") {
        Some(wrap_page(&xml))
    } else {
        None
    }
}

fn generate_architecture_page(project: &Project, xml: &mut String) {
    let mut ids = IdGen::new();

    // Detect services
    let mut svc_nodes: Vec<(u32, String, ServiceType, Option<u16>, String)> = Vec::new();
    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);
        let (svc_type, port) = detect_service_info(dir_path, &svc.command);
        let node_id = ids.next();
        svc_nodes.push((node_id, svc.name.clone(), svc_type, port, svc.command.clone()));
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
    let rows = (svc_count + cols - 1) / cols;
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
            ServiceType::Database => (DATABASE_FILL, DATABASE_STROKE, "shape=cylinder3;boundedLbl=1;backgroundOutline=1;size=12;"),
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

fn generate_api_routes_page(project: &Project, xml: &mut String) {
    let mut all_routes: Vec<(String, Vec<Route>)> = Vec::new();
    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);
        let routes = scan_routes(dir_path);
        if !routes.is_empty() {
            all_routes.push((svc.name.clone(), routes));
        }
    }
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

    for (svc_name, routes) in &all_routes {
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
                _ => ("#f5f5f5", "#666666"),
            };

            let label = format!("{} {}", route.method, esc(&route.path));

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

fn generate_db_models_page(project: &Project, xml: &mut String) {
    let mut all_models: Vec<(String, Vec<(String, String)>)> = Vec::new();

    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);
        collect_db_models(dir_path, &mut all_models);
    }
    let root_clean = strip_win_prefix(&project.path);
    let root = Path::new(&root_clean);
    collect_db_models(root, &mut all_models);

    if all_models.is_empty() {
        return;
    }

    // ── Build adjacency graph for FK relationships ──
    let model_count = all_models.len();
    let model_names: Vec<String> = all_models.iter().map(|(n, _)| n.clone()).collect();

    // Map model name (lowercase) → index
    let name_to_idx: std::collections::HashMap<String, usize> = model_names.iter().enumerate()
        .map(|(i, n)| (n.to_lowercase(), i))
        .collect();

    // Build FK edges: (source_idx, target_idx, field_name)
    let mut fk_links: Vec<(usize, usize)> = Vec::new();
    let mut model_fk_targets: Vec<Vec<(String, usize)>> = vec![Vec::new(); model_count]; // per-model FK targets

    for (idx, (_name, fields)) in all_models.iter().enumerate() {
        for (field_name, field_type) in fields {
            let is_fk = field_type == "FK" || field_type == "M2M"
                || (field_type == "uuid" && (field_name.ends_with("Id") || field_name.ends_with("_id")));
            if is_fk {
                let target = field_name.trim_end_matches("Id").trim_end_matches("_id").to_lowercase();
                if target.is_empty() { continue; }
                // Find target model
                let target_idx = name_to_idx.get(&target)
                    .or_else(|| name_to_idx.get(&format!("{}s", target)))
                    .or_else(|| {
                        name_to_idx.iter()
                            .find(|(k, _)| k.trim_end_matches('s') == target)
                            .map(|(_, v)| v)
                    });
                if let Some(&tidx) = target_idx {
                    if tidx != idx {
                        fk_links.push((idx, tidx));
                        model_fk_targets[idx].push((field_name.clone(), tidx));
                    }
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

    // Build adjacency list
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); model_count];
    for (a, b) in &fk_links {
        if !adj[*a].contains(b) { adj[*a].push(*b); }
        if !adj[*b].contains(a) { adj[*b].push(*a); }
    }

    // BFS ordering: start from most-connected, then expand neighbors
    let mut ordered: Vec<usize> = Vec::with_capacity(model_count);
    let mut visited = vec![false; model_count];

    while ordered.len() < model_count {
        // Pick unvisited with most connections
        let start = (0..model_count)
            .filter(|i| !visited[*i])
            .max_by_key(|i| connection_count[*i])
            .unwrap();

        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start);
        visited[start] = true;

        while let Some(node) = queue.pop_front() {
            ordered.push(node);
            // Sort neighbors by connection count (most connected first)
            let mut neighbors: Vec<usize> = adj[node].iter()
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

    // ── Layout: place in grid following BFS order with dynamic row heights ──
    let mut ids = IdGen::new();
    let cols = 4.min(model_count).max(1);
    let card_w: u32 = 240;
    let row_h: u32 = 22;
    let header_h: u32 = 30;
    let pad: u32 = 10;
    let spacing_x: u32 = 60;
    let spacing_y: u32 = 50;

    // Pre-calculate card heights
    let card_heights: Vec<u32> = all_models.iter()
        .map(|(_, fields)| header_h + pad + (fields.len() as u32) * row_h + pad)
        .collect();

    // Calculate row max heights from the ordered layout
    let num_rows = (model_count + cols - 1) / cols;
    let mut row_max_h: Vec<u32> = vec![0; num_rows];
    for (pos, &model_idx) in ordered.iter().enumerate() {
        let row = pos / cols;
        row_max_h[row] = row_max_h[row].max(card_heights[model_idx]);
    }

    // Calculate cumulative Y positions per row
    let mut row_y: Vec<u32> = vec![40; num_rows];
    for r in 1..num_rows {
        row_y[r] = row_y[r - 1] + row_max_h[r - 1] + spacing_y;
    }

    // Calculate total diagram size
    let total_w = 40 + (cols as u32) * (card_w + spacing_x);
    let total_h = if num_rows > 0 { row_y[num_rows - 1] + row_max_h[num_rows - 1] + 80 } else { 800 };

    xml.push_str("  <diagram id=\"db\" name=\"DB Models\">\n");
    xml.push_str(&format!(
        "    <mxGraphModel dx=\"1422\" dy=\"762\" grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"1\" pageScale=\"1\" pageWidth=\"{}\" pageHeight=\"{}\">\n",
        total_w.max(2400), total_h.max(1600)
    ));
    xml.push_str("      <root>\n");
    xml.push_str("        <mxCell id=\"0\"/>\n");
    xml.push_str("        <mxCell id=\"1\" parent=\"0\"/>\n");

    // Track model_idx → draw.io cell ID
    let mut model_cell_ids: Vec<u32> = vec![0; model_count];

    for (pos, &model_idx) in ordered.iter().enumerate() {
        let (ref model_name, ref fields) = all_models[model_idx];
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
            group_id, esc(model_name), header_h
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>\n",
            x, y, card_w, card_h
        ));
        xml.push_str("        </mxCell>\n");

        for (fi, (field_name, field_type)) in fields.iter().enumerate() {
            let fid = ids.next();
            let fy = header_h + pad + (fi as u32) * row_h;

            let is_fk = field_type == "FK" || field_type == "M2M"
                || (field_type == "uuid" && (field_name.ends_with("Id") || field_name.ends_with("_id")));
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

    // ── Draw FK edges with curved routing ──
    for (source_idx, targets) in model_fk_targets.iter().enumerate() {
        let source_id = model_cell_ids[source_idx];
        for (_field_name, target_idx) in targets {
            let target_id = model_cell_ids[*target_idx];
            if source_id == 0 || target_id == 0 || source_id == target_id { continue; }

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

/// Collect DB models from a directory (reusing db_models scanning logic).
fn collect_db_models(dir: &Path, models: &mut Vec<(String, Vec<(String, String)>)>) {
    // Scan Sequelize, Python, Go, Prisma files in the directory and known subdirs
    scan_db_files(dir, models);

    let model_dir_names = ["models", "db", "database", "schema", "entities", "entity"];
    for base in &["", "src", "app", "lib"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        if let Ok(entries) = std::fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if model_dir_names.contains(&name.as_str()) {
                        scan_db_files(&entry.path(), models);
                    }
                }
            }
        }
    }

    // Prisma
    let prisma_path = dir.join("prisma").join("schema.prisma");
    if let Ok(content) = std::fs::read_to_string(&prisma_path) {
        parse_prisma_models(&content, models);
    }
}

fn scan_db_files(dir: &Path, models: &mut Vec<(String, Vec<(String, String)>)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if let Ok(content) = std::fs::read_to_string(&path) {
            match ext {
                "js" | "ts" | "mjs" => parse_sequelize_models_drawio(&content, models),
                "py" => parse_python_models_drawio(&content, models),
                "go" => parse_gorm_models_drawio(&content, models),
                _ => {}
            }
        }
    }
}

fn parse_sequelize_models_drawio(content: &str, models: &mut Vec<(String, Vec<(String, String)>)>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_define = trimmed.contains(".define(") || trimmed.contains(".define<");

        if is_define {
            // Extract model name (this line or next few lines)
            let model_name = extract_quoted_str(trimmed)
                .or_else(|| {
                    for la in 1..=3 {
                        if i + la < lines.len() {
                            if let Some(n) = extract_quoted_str(lines[i + la].trim()) {
                                return Some(n);
                            }
                        }
                    }
                    None
                });

            if let Some(name) = model_name {
                if !name.is_empty() && !models.iter().any(|(n, _)| n == &name) {
                    let mut fields = Vec::new();
                    let mut j = i + 1;
                    let mut brace_depth = 0i32;
                    let mut in_fields = false;
                    let mut current_field: Option<String> = None;
                    let mut field_depth = 0i32;

                    while j < lines.len() {
                        let fl = lines[j].trim();
                        let open = fl.matches('{').count() as i32;
                        let close = fl.matches('}').count() as i32;
                        brace_depth += open;
                        brace_depth -= close;

                        if !in_fields && open > 0 { in_fields = true; j += 1; continue; }
                        if in_fields && brace_depth <= 0 { break; }

                        // Single-line field with DataTypes
                        if brace_depth >= 1 && (fl.contains(": {") || fl.contains(":{")) {
                            if let Some(cpos) = fl.find(':') {
                                let candidate = fl[..cpos].trim().trim_matches('\'').trim_matches('"').to_string();
                                if !candidate.is_empty() && !is_meta_key(&candidate) {
                                    if let Some(dt) = extract_dt(fl) {
                                        fields.push((candidate, dt));
                                    } else {
                                        current_field = Some(candidate);
                                        field_depth = brace_depth;
                                    }
                                }
                            }
                        } else if current_field.is_some() && (fl.contains("DataTypes.") || fl.contains("DataType.")) {
                            if fl.trim_start().starts_with("type:") || fl.trim_start().starts_with("type :") {
                                if let Some(dt) = extract_dt(fl) {
                                    if let Some(name) = current_field.take() {
                                        fields.push((name, dt));
                                    }
                                }
                            }
                        }

                        if current_field.is_some() && brace_depth < field_depth {
                            current_field = None;
                        }

                        j += 1;
                    }
                    if !fields.is_empty() {
                        models.push((name, fields));
                    }
                    i = j;
                    continue;
                }
            }
        }
        i += 1;
    }
}

fn parse_python_models_drawio(content: &str, models: &mut Vec<(String, Vec<(String, String)>)>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("class ") && (trimmed.contains("(Base)") || trimmed.contains("db.Model") || trimmed.contains("models.Model")) {
            let class_name = trimmed.strip_prefix("class ")
                .and_then(|s| s.split('(').next())
                .unwrap_or("").trim().to_string();

            if class_name.is_empty() || models.iter().any(|(n, _)| n == &class_name) {
                i += 1; continue;
            }

            let mut fields = Vec::new();
            i += 1;
            while i < lines.len() {
                let fl = lines[i].trim();
                if !lines[i].starts_with(' ') && !lines[i].starts_with('\t') && !fl.is_empty() { break; }
                if fl.contains("Column(") || fl.contains("column(") || fl.contains("models.") {
                    if let Some(eq) = fl.find('=') {
                        let name = fl[..eq].trim().to_string();
                        if !name.starts_with('_') && !name.starts_with('#') && name != "class" && name != "Meta" {
                            let rest = &fl[eq + 1..];
                            let ft = detect_python_field_type(rest);
                            fields.push((name, ft));
                        }
                    }
                }
                i += 1;
            }
            if !fields.is_empty() { models.push((class_name, fields)); }
            continue;
        }
        i += 1;
    }
}

fn parse_gorm_models_drawio(content: &str, models: &mut Vec<(String, Vec<(String, String)>)>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("type ") && trimmed.contains("struct") && trimmed.ends_with('{') {
            let struct_name = trimmed.strip_prefix("type ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("").to_string();
            i += 1;
            let mut fields = Vec::new();
            let mut is_gorm = false;
            while i < lines.len() {
                let fl = lines[i].trim();
                if fl == "}" { break; }
                if fl.contains("gorm.Model") || fl.contains("gorm:\"") { is_gorm = true; }
                let parts: Vec<&str> = fl.split_whitespace().collect();
                if parts.len() >= 2 && parts[0].chars().next().map(|c| c.is_uppercase()).unwrap_or(false) && parts[0] != "gorm" {
                    let go_type = parts[1].trim_start_matches('*');
                    let mapped = match go_type {
                        "string" => "string", "int" | "int32" | "int64" => "int",
                        "float32" | "float64" => "float", "bool" => "bool",
                        "time.Time" => "datetime", _ => "FK",
                    };
                    fields.push((parts[0].to_string(), mapped.to_string()));
                }
                i += 1;
            }
            if is_gorm && !fields.is_empty() && !struct_name.is_empty() && !models.iter().any(|(n,_)| n == &struct_name) {
                models.push((struct_name, fields));
            }
            continue;
        }
        i += 1;
    }
}

fn parse_prisma_models(content: &str, models: &mut Vec<(String, Vec<(String, String)>)>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("model ") && trimmed.ends_with('{') {
            let model_name = trimmed.strip_prefix("model ").and_then(|s| s.strip_suffix('{')).unwrap_or("").trim().to_string();
            let mut fields = Vec::new();
            i += 1;
            while i < lines.len() {
                let fl = lines[i].trim();
                if fl == "}" { break; }
                if fl.is_empty() || fl.starts_with("//") || fl.starts_with("@@") { i += 1; continue; }
                let parts: Vec<&str> = fl.split_whitespace().collect();
                if parts.len() >= 2 {
                    let pt = parts[1].trim_end_matches('?').trim_end_matches("[]").to_lowercase();
                    let mapped = match pt.as_str() {
                        "string" => "string", "int" | "bigint" => "int", "float" | "decimal" => "float",
                        "boolean" => "bool", "datetime" => "datetime", "json" => "json", _ => "FK",
                    };
                    fields.push((parts[0].to_string(), mapped.to_string()));
                }
                i += 1;
            }
            if !fields.is_empty() { models.push((model_name, fields)); }
        }
        i += 1;
    }
}

fn extract_quoted_str(line: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        if let Some(start) = line.find(quote) {
            let rest = &line[start + 1..];
            if let Some(end) = rest.find(quote) {
                let val = &rest[..end];
                if !val.is_empty() && !val.contains(' ') { return Some(val.to_string()); }
            }
        }
    }
    None
}

fn extract_dt(line: &str) -> Option<String> {
    let dt_pos = line.find("DataTypes.").or_else(|| line.find("DataType."))?;
    let after = &line[dt_pos..];
    let type_str = after.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_').next().unwrap_or("");
    let mapped = if type_str.contains("STRING") || type_str.contains("TEXT") || type_str.contains("CHAR") { "string" }
    else if type_str.contains("INTEGER") || type_str.contains("BIGINT") || type_str.contains("SMALLINT") { "int" }
    else if type_str.contains("FLOAT") || type_str.contains("DOUBLE") || type_str.contains("DECIMAL") { "float" }
    else if type_str.contains("BOOLEAN") { "bool" }
    else if type_str.contains("DATE") { "datetime" }
    else if type_str.contains("JSON") { "json" }
    else if type_str.contains("UUID") { "uuid" }
    else if type_str.contains("ENUM") { "enum" }
    else if type_str.contains("ARRAY") { "array" }
    else if type_str.contains("BLOB") || type_str.contains("BINARY") { "binary" }
    else { "string" };
    Some(mapped.to_string())
}

fn is_meta_key(name: &str) -> bool {
    matches!(name, "type" | "allowNull" | "defaultValue" | "primaryKey" | "autoIncrement"
        | "references" | "get" | "set" | "validate" | "unique" | "comment" | "field"
        | "onDelete" | "onUpdate")
}

fn detect_python_field_type(rest: &str) -> String {
    if rest.contains("String") || rest.contains("Text") || rest.contains("CharField") || rest.contains("TextField") { "string".into() }
    else if rest.contains("Integer") || rest.contains("IntegerField") { "int".into() }
    else if rest.contains("Float") || rest.contains("DecimalField") { "float".into() }
    else if rest.contains("Boolean") || rest.contains("BooleanField") { "bool".into() }
    else if rest.contains("DateTime") || rest.contains("DateField") { "datetime".into() }
    else if rest.contains("ForeignKey") || rest.contains("OneToOneField") { "FK".into() }
    else if rest.contains("ManyToManyField") { "M2M".into() }
    else if rest.contains("JSON") { "json".into() }
    else { "string".into() }
}

// ── Service detection ──

fn detect_service_info(dir: &Path, command: &str) -> (ServiceType, Option<u16>) {
    let cmd_lower = command.to_lowercase();

    if cmd_lower.contains("npm run dev") || cmd_lower.contains("yarn dev") || cmd_lower.contains("vite") || cmd_lower.contains("next") {
        return (ServiceType::Frontend, extract_port(&cmd_lower).or(Some(3000)));
    }
    if cmd_lower.contains("uvicorn") || cmd_lower.contains("gunicorn") || cmd_lower.contains("flask") {
        return (ServiceType::Backend, extract_port(&cmd_lower).or(Some(8000)));
    }
    if cmd_lower.contains("django") || cmd_lower.contains("manage.py") {
        return (ServiceType::Backend, extract_port(&cmd_lower).or(Some(8000)));
    }
    if cmd_lower.contains("cargo run") || cmd_lower.contains("go run") {
        return (ServiceType::Backend, extract_port(&cmd_lower));
    }
    if cmd_lower.starts_with("python ") || cmd_lower.starts_with("python3 ") {
        return (ServiceType::Backend, extract_port(&cmd_lower).or(Some(8000)));
    }
    if cmd_lower.contains("celery") || cmd_lower.contains("worker") {
        return (ServiceType::Worker, None);
    }

    if dir.join("package.json").exists() {
        if let Ok(content) = std::fs::read_to_string(dir.join("package.json")) {
            let lower = content.to_lowercase();
            if lower.contains("react") || lower.contains("vue") || lower.contains("svelte")
                || lower.contains("next") || lower.contains("vite") || lower.contains("nuxt")
            {
                return (ServiceType::Frontend, Some(3000));
            }
            if lower.contains("express") || lower.contains("fastify") || lower.contains("nest") {
                return (ServiceType::Backend, Some(3000));
            }
        }
        return (ServiceType::Frontend, Some(3000));
    }
    if dir.join("requirements.txt").exists() || dir.join("pyproject.toml").exists() {
        return (ServiceType::Backend, Some(8000));
    }

    (ServiceType::Unknown, None)
}

fn extract_port(cmd: &str) -> Option<u16> {
    for pat in &["--port ", "-p "] {
        if let Some(pos) = cmd.find(pat) {
            let rest = &cmd[pos + pat.len()..];
            let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(port) = port_str.parse() {
                return Some(port);
            }
        }
    }
    if let Some(pos) = cmd.rfind(':') {
        let rest = &cmd[pos + 1..];
        let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if port_str.len() >= 4 {
            if let Ok(port) = port_str.parse() {
                return Some(port);
            }
        }
    }
    None
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
        // Check .env files safely (keys only, never read values)
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
        // docker-compose is safe to read (not a credentials file)
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

// ── Route scanning ──

struct Route {
    method: String,
    path: String,
}

fn scan_routes(dir: &Path) -> Vec<Route> {
    let mut routes = Vec::new();
    scan_python_routes(dir, &mut routes);
    scan_node_routes(dir, &mut routes);
    routes
}

fn scan_python_routes(dir: &Path, routes: &mut Vec<Route>) {
    let py_files = ["main.py", "app.py", "server.py", "routes.py", "views.py", "api.py"];
    for filename in &py_files {
        if let Ok(content) = std::fs::read_to_string(dir.join(filename)) {
            for line in content.lines() {
                if let Some(r) = parse_python_decorator(line.trim()) {
                    routes.push(r);
                }
            }
        }
    }
    let route_dir_names = ["routers", "routes", "api", "endpoints"];
    for base in &["", "src"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        for sub in find_subdirs_ci(&search_dir, &route_dir_names) {
            if let Ok(entries) = std::fs::read_dir(&sub) {
                for entry in entries.flatten() {
                    if entry.path().extension().map(|e| e == "py").unwrap_or(false) {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            for line in content.lines() {
                                if let Some(r) = parse_python_decorator(line.trim()) {
                                    routes.push(r);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn parse_python_decorator(line: &str) -> Option<Route> {
    let methods = [
        ("get(", "GET"), ("post(", "POST"), ("put(", "PUT"),
        ("delete(", "DELETE"), ("patch(", "PATCH"), ("websocket(", "WS"),
    ];
    if !line.starts_with('@') { return None; }
    for (pattern, method) in &methods {
        if let Some(pos) = line.find(pattern) {
            let rest = &line[pos + pattern.len()..];
            let path = extract_string_arg(rest)?;
            return Some(Route { method: method.to_string(), path });
        }
    }
    if let Some(pos) = line.find("route(") {
        let rest = &line[pos + 6..];
        let path = extract_string_arg(rest)?;
        return Some(Route { method: "GET".to_string(), path });
    }
    None
}

fn scan_node_routes(dir: &Path, routes: &mut Vec<Route>) {
    let files = ["index.js", "index.ts", "app.js", "app.ts", "server.js", "server.ts", "routes.js", "routes.ts"];
    for filename in &files {
        if let Ok(content) = std::fs::read_to_string(dir.join(filename)) {
            for line in content.lines() {
                if let Some(r) = parse_express_route(line.trim()) { routes.push(r); }
            }
        }
    }
    let route_dir_names = ["routes", "api", "routers"];
    for base in &["", "src"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        for sub in find_subdirs_ci(&search_dir, &route_dir_names) {
            if let Ok(entries) = std::fs::read_dir(&sub) {
                for entry in entries.flatten() {
                    let ext = entry.path().extension().map(|e| e.to_string_lossy().to_string());
                    if matches!(ext.as_deref(), Some("js") | Some("ts")) {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            for line in content.lines() {
                                if let Some(r) = parse_express_route(line.trim()) { routes.push(r); }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn parse_express_route(line: &str) -> Option<Route> {
    let methods = [
        (".get(", "GET"), (".post(", "POST"), (".put(", "PUT"),
        (".delete(", "DELETE"), (".patch(", "PATCH"),
    ];
    for (pattern, method) in &methods {
        if let Some(pos) = line.find(pattern) {
            let rest = &line[pos + pattern.len()..];
            let path = extract_string_arg(rest)?;
            let before = &line[..pos];
            if before.contains("require") || before.contains("import") { continue; }
            return Some(Route { method: method.to_string(), path });
        }
    }
    None
}

fn extract_string_arg(s: &str) -> Option<String> {
    let trimmed = s.trim();
    let quote = if trimmed.starts_with('"') { '"' }
    else if trimmed.starts_with('\'') { '\'' }
    else if trimmed.starts_with('`') { '`' }
    else { return None; };
    let rest = &trimmed[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Find subdirectories of `dir` matching any of `names` (case-insensitive).
fn find_subdirs_ci(dir: &Path, names: &[&str]) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let dirname = entry.file_name().to_string_lossy().to_lowercase();
                if names.iter().any(|n| *n == dirname) {
                    result.push(entry.path());
                }
            }
        }
    }
    result
}

/// XML-escape a string for use in attribute values.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
