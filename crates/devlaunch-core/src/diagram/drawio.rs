//! Draw.io (.drawio) XML diagram generation.
//!
//! Generates architecture diagrams in draw.io format that can be opened
//! in diagrams.net, VS Code Draw.io extension, or any compatible editor.

use std::path::Path;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Colors for service types
struct Theme {
    frontend_fill: &'static str,
    frontend_stroke: &'static str,
    backend_fill: &'static str,
    backend_stroke: &'static str,
    database_fill: &'static str,
    database_stroke: &'static str,
    worker_fill: &'static str,
    worker_stroke: &'static str,
    external_fill: &'static str,
    external_stroke: &'static str,
    container_fill: &'static str,
    container_stroke: &'static str,
}

const THEME: Theme = Theme {
    frontend_fill: "#d5e8d4",
    frontend_stroke: "#82b366",
    backend_fill: "#dae8fc",
    backend_stroke: "#6c8ebf",
    database_fill: "#fff2cc",
    database_stroke: "#d6b656",
    worker_fill: "#e1d5e7",
    worker_stroke: "#9673a6",
    external_fill: "#f5f5f5",
    external_stroke: "#666666",
    container_fill: "#F0F4FF",
    container_stroke: "#4A6FA5",
};

/// Cell ID generator
struct IdGen(u32);

impl IdGen {
    fn new() -> Self {
        IdGen(10) // start after reserved IDs
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

struct ServiceNode {
    id: u32,
    name: String,
    svc_type: ServiceType,
    port: Option<u16>,
    command: String,
}

struct ExternalNode {
    id: u32,
    name: String,
}

/// Generate a complete draw.io XML architecture diagram.
pub fn generate_architecture(project: &Project) -> String {
    let mut ids = IdGen::new();
    let container_id = ids.next();
    let mut services: Vec<ServiceNode> = Vec::new();
    let mut externals: Vec<ExternalNode> = Vec::new();
    let mut connections: Vec<(u32, u32, &str)> = Vec::new(); // (from, to, label)

    // Detect services
    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);

        let (svc_type, port) = detect_service_info(dir_path, &svc.command);
        let node_id = ids.next();

        services.push(ServiceNode {
            id: node_id,
            name: svc.name.clone(),
            svc_type,
            port,
            command: svc.command.clone(),
        });
    }

    // Detect connections (frontend → backend)
    for svc in &services {
        if matches!(svc.svc_type, ServiceType::Frontend) {
            for other in &services {
                if matches!(other.svc_type, ServiceType::Backend) {
                    connections.push((svc.id, other.id, "API"));
                }
            }
        }
    }

    // Detect external services
    let root = strip_win_prefix(&project.path);
    let root_path = Path::new(&root);
    let external_names = detect_external_services(root_path, project);
    for name in &external_names {
        let ext_id = ids.next();
        externals.push(ExternalNode {
            id: ext_id,
            name: name.clone(),
        });
        // Connect backends to external services
        for svc in &services {
            if matches!(svc.svc_type, ServiceType::Backend) {
                connections.push((svc.id, ext_id, ""));
            }
        }
    }

    // Layout calculation
    let svc_count = services.len();
    let svc_width: u32 = 160;
    let svc_height: u32 = 70;
    let svc_spacing: u32 = 40;
    let padding: u32 = 40;
    let header_height: u32 = 30;

    let cols = if svc_count <= 3 { svc_count } else { 3 };
    let rows = (svc_count + cols - 1) / cols;

    let container_w = (cols as u32) * svc_width + (cols as u32 + 1) * svc_spacing;
    let container_h = header_height + padding + (rows as u32) * svc_height + (rows as u32) * svc_spacing;

    let container_x: u32 = 80;
    let container_y: u32 = 60;

    // Build XML
    let mut xml = String::new();
    xml.push_str(r#"<mxfile host="devlaunch" modified="2026-01-01T00:00:00.000Z" agent="devlaunch-rs" version="1.0">"#);
    xml.push('\n');
    xml.push_str(r#"  <diagram id="architecture" name="Architecture">"#);
    xml.push('\n');

    let page_w = container_w + 400;
    let page_h = container_h + 200;
    xml.push_str(&format!(
        r#"    <mxGraphModel dx="{}" dy="{}" grid="1" gridSize="10" guides="1" tooltips="1" connect="1" arrows="1" fold="1" page="1" pageScale="1" pageWidth="{}" pageHeight="{}">"#,
        page_w, page_h, page_w.max(1169), page_h.max(827)
    ));
    xml.push('\n');
    xml.push_str("      <root>\n");
    xml.push_str("        <mxCell id=\"0\" />\n");
    xml.push_str("        <mxCell id=\"1\" parent=\"0\" />\n");

    // Container
    xml.push_str(&format!(
        "        <mxCell id=\"{}\" value=\"{}\" style=\"rounded=1;whiteSpace=wrap;html=1;container=1;collapsible=0;fillColor={};strokeColor={};fontStyle=1;verticalAlign=top;fontSize=14;spacingTop=5;arcSize=6;strokeWidth=2;\" vertex=\"1\" parent=\"1\">\n",
        container_id,
        xml_escape(&project.name),
        THEME.container_fill,
        THEME.container_stroke,
    ));
    xml.push_str(&format!(
        "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\" />\n",
        container_x, container_y, container_w, container_h
    ));
    xml.push_str("        </mxCell>\n");

    // Service nodes inside container
    for (i, svc) in services.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let x = svc_spacing + (col as u32) * (svc_width + svc_spacing);
        let y = header_height + svc_spacing + (row as u32) * (svc_height + svc_spacing);

        let (fill, stroke, shape) = match svc.svc_type {
            ServiceType::Frontend => (THEME.frontend_fill, THEME.frontend_stroke, "rounded=1"),
            ServiceType::Backend => (THEME.backend_fill, THEME.backend_stroke, "rounded=1"),
            ServiceType::Database => (THEME.database_fill, THEME.database_stroke, "shape=cylinder3;boundedLbl=1;backgroundOutline=1;size=12"),
            ServiceType::Worker => (THEME.worker_fill, THEME.worker_stroke, "rounded=1"),
            ServiceType::Unknown => (THEME.external_fill, THEME.external_stroke, "rounded=1"),
        };

        let icon = match svc.svc_type {
            ServiceType::Frontend => "🌐 ",
            ServiceType::Backend => "⚙️ ",
            ServiceType::Database => "🗄️ ",
            ServiceType::Worker => "⚡ ",
            ServiceType::Unknown => "📦 ",
        };

        let type_label = match svc.svc_type {
            ServiceType::Frontend => "Frontend",
            ServiceType::Backend => "API",
            ServiceType::Database => "Database",
            ServiceType::Worker => "Worker",
            ServiceType::Unknown => &svc.command,
        };

        let port_str = svc.port.map(|p| format!(" :{}", p)).unwrap_or_default();
        let label = format!("{}{}{}&lt;br&gt;&lt;font style=&quot;font-size:10px&quot;&gt;{}&lt;/font&gt;", icon, xml_escape(&svc.name), port_str, type_label);

        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"{};whiteSpace=wrap;html=1;fillColor={};strokeColor={};fontColor=#333333;fontSize=13;fontStyle=1;strokeWidth=1.5;\" vertex=\"1\" parent=\"{}\">\n",
            svc.id, label, shape, fill, stroke, container_id
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\" />\n",
            x, y, svc_width, svc_height
        ));
        xml.push_str("        </mxCell>\n");
    }

    // External nodes (outside container)
    let ext_x = container_x + container_w + 80;
    for (i, ext) in externals.iter().enumerate() {
        let ext_y = container_y + 20 + (i as u32) * 90;
        let icon = match ext.name.as_str() {
            "PostgreSQL" | "MongoDB" => "🗄️ ",
            "Redis" => "⚡ ",
            "Ollama" => "🦙 ",
            "AI API" => "🤖 ",
            "AWS S3" => "☁️ ",
            "RabbitMQ" => "🐰 ",
            _ => "🔗 ",
        };
        let label = format!("{}{}", icon, xml_escape(&ext.name));
        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"rounded=1;whiteSpace=wrap;html=1;fillColor={};strokeColor={};fontColor=#333333;fontSize=12;fontStyle=1;dashed=1;dashPattern=8 4;strokeWidth=1.5;\" vertex=\"1\" parent=\"1\">\n",
            ext.id, label, THEME.external_fill, THEME.external_stroke
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"{}\" width=\"140\" height=\"60\" as=\"geometry\" />\n",
            ext_x, ext_y
        ));
        xml.push_str("        </mxCell>\n");
    }

    // Connections
    for (i, (from, to, label)) in connections.iter().enumerate() {
        let edge_id = ids.next();
        let is_external = externals.iter().any(|e| e.id == *to);
        let (style, parent) = if is_external {
            ("endArrow=classic;html=1;rounded=1;curved=1;strokeWidth=1.5;strokeColor=#999999;dashed=1;dashPattern=8 4;", "1")
        } else {
            ("endArrow=classic;html=1;rounded=1;curved=1;strokeWidth=2;strokeColor=#333333;", &*container_id.to_string())
        };

        let label_attr = if label.is_empty() {
            String::new()
        } else {
            format!("value=\"{}\" ", label)
        };

        // For cross-container edges, parent must be "1"
        let parent_val = if is_external { "1" } else { parent };
        let _ = i; // suppress warning

        xml.push_str(&format!(
            "        <mxCell id=\"{}\" {}style=\"{}\" edge=\"1\" source=\"{}\" target=\"{}\" parent=\"{}\">\n",
            edge_id, label_attr, style, from, to, parent_val
        ));
        xml.push_str("          <mxGeometry relative=\"1\" as=\"geometry\" />\n");
        xml.push_str("        </mxCell>\n");
    }

    xml.push_str("      </root>\n");
    xml.push_str("    </mxGraphModel>\n");
    xml.push_str("  </diagram>\n");
    xml.push_str("</mxfile>\n");

    xml
}

/// Generate draw.io XML for API routes.
pub fn generate_api_routes(project: &Project) -> String {
    // Reuse the route scanning from api_routes module
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
        return String::new();
    }

    let mut ids = IdGen::new();
    let mut xml = String::new();
    xml.push_str(r#"<mxfile host="devlaunch" modified="2026-01-01T00:00:00.000Z" agent="devlaunch-rs" version="1.0">"#);
    xml.push('\n');
    xml.push_str(r#"  <diagram id="api-routes" name="API Routes">"#);
    xml.push('\n');
    xml.push_str(r#"    <mxGraphModel dx="1200" dy="900" grid="1" gridSize="10" guides="1" tooltips="1" connect="1" arrows="1" fold="1" page="1" pageScale="1" pageWidth="1600" pageHeight="1200">"#);
    xml.push('\n');
    xml.push_str("      <root>\n");
    xml.push_str("        <mxCell id=\"0\" />\n");
    xml.push_str("        <mxCell id=\"1\" parent=\"0\" />\n");

    let mut group_x: u32 = 40;

    for (svc_name, routes) in &all_routes {
        let group_id = ids.next();
        let route_h: u32 = 40;
        let route_spacing: u32 = 8;
        let route_w: u32 = 300;
        let header_h: u32 = 35;
        let padding: u32 = 20;
        let group_h = header_h + padding + (routes.len() as u32) * (route_h + route_spacing) + padding;
        let group_w = route_w + 2 * padding;

        // Swimlane container for service
        xml.push_str(&format!(
            "        <mxCell id=\"{}\" value=\"{}\" style=\"swimlane;startSize={};fillColor=#dae8fc;strokeColor=#6c8ebf;fontStyle=1;fontSize=13;rounded=1;arcSize=8;\" vertex=\"1\" parent=\"1\">\n",
            group_id, xml_escape(svc_name), header_h
        ));
        xml.push_str(&format!(
            "          <mxGeometry x=\"{}\" y=\"40\" width=\"{}\" height=\"{}\" as=\"geometry\" />\n",
            group_x, group_w, group_h
        ));
        xml.push_str("        </mxCell>\n");

        for (i, route) in routes.iter().enumerate() {
            let route_id = ids.next();
            let y = header_h + padding + (i as u32) * (route_h + route_spacing);

            let (fill, stroke) = match route.method.as_str() {
                "GET" => ("#d5e8d4", "#82b366"),
                "POST" => ("#fff2cc", "#d6b656"),
                "PUT" => ("#dae8fc", "#6c8ebf"),
                "DELETE" => ("#f8cecc", "#b85450"),
                "PATCH" => ("#e1d5e7", "#9673a6"),
                "WS" => ("#ffe6cc", "#d79b00"),
                _ => ("#f5f5f5", "#666666"),
            };

            let method_badge = &route.method;
            let label = format!(
                "&lt;b&gt;{}&lt;/b&gt;  {}",
                method_badge,
                xml_escape(&route.path),
            );

            xml.push_str(&format!(
                "        <mxCell id=\"{}\" value=\"{}\" style=\"rounded=1;whiteSpace=wrap;html=1;fillColor={};strokeColor={};fontColor=#333333;fontSize=11;align=left;spacingLeft=10;\" vertex=\"1\" parent=\"{}\">\n",
                route_id, label, fill, stroke, group_id
            ));
            xml.push_str(&format!(
                "          <mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\" />\n",
                padding, y, route_w, route_h
            ));
            xml.push_str("        </mxCell>\n");
        }

        group_x += group_w + 40;
    }

    xml.push_str("      </root>\n");
    xml.push_str("    </mxGraphModel>\n");
    xml.push_str("  </diagram>\n");
    xml.push_str("</mxfile>\n");

    xml
}

/// Generate a multi-page draw.io file with architecture + API routes.
pub fn generate_all(project: &Project) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<mxfile host="devlaunch" modified="2026-01-01T00:00:00.000Z" agent="devlaunch-rs" version="1.0" pages="2">"#);
    xml.push('\n');

    // Page 1: Architecture
    let arch = generate_architecture_inner(project);
    xml.push_str(&arch);

    // Page 2: API Routes (if any)
    let api = generate_api_routes_inner(project);
    if !api.is_empty() {
        xml.push_str(&api);
    }

    xml.push_str("</mxfile>\n");
    xml
}

// ── Internal generators that produce <diagram> blocks (no <mxfile> wrapper) ──

fn generate_architecture_inner(project: &Project) -> String {
    let full = generate_architecture(project);
    // Extract the <diagram>...</diagram> block
    if let Some(start) = full.find("<diagram") {
        if let Some(end) = full.find("</diagram>") {
            return full[start..end + 10].to_string() + "\n";
        }
    }
    String::new()
}

fn generate_api_routes_inner(project: &Project) -> String {
    let full = generate_api_routes(project);
    if full.is_empty() {
        return String::new();
    }
    if let Some(start) = full.find("<diagram") {
        if let Some(end) = full.find("</diagram>") {
            return full[start..end + 10].to_string() + "\n";
        }
    }
    String::new()
}

// ── Service detection (reused from architecture.rs logic) ──

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

    // Check directory contents
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
        for env_file in &[".env", ".env.example"] {
            if let Ok(content) = std::fs::read_to_string(dir.join(env_file)) {
                let upper = content.to_uppercase();
                check_add(&mut externals, &upper, "POSTGRES", "PostgreSQL");
                check_add(&mut externals, &upper, "DATABASE_URL", "PostgreSQL");
                check_add(&mut externals, &upper, "REDIS", "Redis");
                check_add(&mut externals, &upper, "MONGO", "MongoDB");
                check_add(&mut externals, &upper, "OLLAMA", "Ollama");
                check_add(&mut externals, &upper, "OPENAI", "AI API");
                check_add(&mut externals, &upper, "ANTHROPIC", "AI API");
                check_add(&mut externals, &upper, "S3", "AWS S3");
            }
        }

        for compose in &["docker-compose.yml", "docker-compose.yaml", "compose.yml"] {
            if let Ok(content) = std::fs::read_to_string(dir.join(compose)) {
                let lower = content.to_lowercase();
                check_add(&mut externals, &lower, "postgres", "PostgreSQL");
                check_add(&mut externals, &lower, "redis", "Redis");
                check_add(&mut externals, &lower, "mongo", "MongoDB");
                check_add(&mut externals, &lower, "rabbitmq", "RabbitMQ");
            }
        }
    }

    externals
}

fn check_add(list: &mut Vec<String>, haystack: &str, keyword: &str, service: &str) {
    if haystack.contains(keyword) && !list.contains(&service.to_string()) {
        list.push(service.to_string());
    }
}

// ── Route scanning (duplicated from api_routes.rs to avoid coupling) ──

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
    if !line.starts_with('@') {
        return None;
    }
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
                if let Some(r) = parse_express_route(line.trim()) {
                    routes.push(r);
                }
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
                                if let Some(r) = parse_express_route(line.trim()) {
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
            if before.contains("require") || before.contains("import") {
                continue;
            }
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

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
