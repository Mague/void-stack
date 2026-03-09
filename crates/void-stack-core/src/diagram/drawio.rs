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

/// Generate a multi-page draw.io file with architecture + API routes.
pub fn generate_all(project: &Project) -> String {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<mxfile host=\"void-stack\" agent=\"void-stack\" version=\"1.0\">\n");

    // Page 1: Architecture
    generate_architecture_page(project, &mut xml);

    // Page 2: API Routes (if any)
    generate_api_routes_page(project, &mut xml);

    xml.push_str("</mxfile>\n");
    xml
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
    for subdir in &["routers", "routes", "api", "endpoints"] {
        let sub = dir.join(subdir);
        if sub.is_dir() {
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
    for subdir in &["routes", "api"] {
        let sub = dir.join(subdir);
        if sub.is_dir() {
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

/// XML-escape a string for use in attribute values.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
