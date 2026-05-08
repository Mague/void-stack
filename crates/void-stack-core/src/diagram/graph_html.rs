//! Self-contained interactive `graph.html` generator.
//!
//! Builds a dependency graph for the project, attaches CC + Leiden community
//! metadata when available, and writes a single HTML file with Cytoscape.js
//! embedded inline (no CDN). Output goes to `{project_path}/void-stack-out/graph.html`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::analyzer::complexity::analyze_file;
use crate::analyzer::graph::{ArchLayer, DependencyGraph, Language, ModuleNode};
use crate::analyzer::imports::build_graph;
use crate::model::Project;

/// Cytoscape.js minified, embedded at compile time.
const CYTOSCAPE_JS: &str = include_str!("./cytoscape.min.js");

/// Layer color palette for nodes.
const COLOR_CONTROLLER: &str = "#7F77DD";
const COLOR_SERVICE: &str = "#1D9E75";
const COLOR_REPOSITORY: &str = "#D85A30";
const COLOR_MODEL: &str = "#378ADD";
const COLOR_UNKNOWN: &str = "#888780";

/// Community palette (cycles every 6).
const COMMUNITY_COLORS: [&str; 6] = [
    "#00E5FF", // cyan
    "#FFB400", // amber
    "#FF4FB7", // pink
    "#7CFF6B", // green
    "#FF6B6B", // red
    "#B084FF", // purple
];

/// Threshold for simplification when graph is large.
const LARGE_GRAPH_THRESHOLD: usize = 500;

/// Color hex code for an architectural layer.
pub fn layer_color(layer: ArchLayer) -> &'static str {
    match layer {
        ArchLayer::Controller => COLOR_CONTROLLER,
        ArchLayer::Service => COLOR_SERVICE,
        ArchLayer::Repository => COLOR_REPOSITORY,
        ArchLayer::Model => COLOR_MODEL,
        // Utility / Config / Test / Unknown collapse to gray.
        _ => COLOR_UNKNOWN,
    }
}

/// Color for community id, cycling through [`COMMUNITY_COLORS`].
pub fn community_color(community: usize) -> &'static str {
    COMMUNITY_COLORS[community % COMMUNITY_COLORS.len()]
}

/// Generate `graph.html` for `project`, return the absolute path it was written to.
pub fn generate_graph_html(project: &Project) -> Result<PathBuf, String> {
    let root = Path::new(&project.path);
    let graph = build_graph(root)
        .ok_or_else(|| "Could not build dependency graph for project".to_string())?;

    let coupling = graph.coupling_metrics();
    let cc_map = compute_cc_map(&graph, root);
    let community_map = load_module_communities(project, &graph);

    let nodes_json = build_nodes_json(&graph, &coupling, &cc_map, &community_map);

    // Mirror the same visibility filter build_nodes_json applies, so edges
    // referencing dropped nodes don't sneak into the JSON and crash
    // Cytoscape with "nonexistent target/source".
    let visible_ids: std::collections::HashSet<&str> =
        if graph.modules.len() > LARGE_GRAPH_THRESHOLD {
            graph
                .modules
                .iter()
                .filter(|m| {
                    let (fan_in, fan_out) = coupling.get(&m.path).copied().unwrap_or((0, 0));
                    let cc = cc_map.get(&m.path).copied().unwrap_or(0);
                    fan_in > 1 || fan_out > 1 || cc > 5
                })
                .map(|m| m.path.as_str())
                .collect()
        } else {
            graph.modules.iter().map(|m| m.path.as_str()).collect()
        };

    let edges_json = build_edges_json(&graph, &visible_ids);
    let total_nodes = graph.modules.len();
    let shown_nodes = nodes_json.matches("\"id\":").count();

    let html = render_html(
        &project.name,
        &nodes_json,
        &edges_json,
        total_nodes,
        shown_nodes,
    );

    let out_dir = root.join("void-stack-out");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create void-stack-out: {}", e))?;
    let out_path = out_dir.join("graph.html");
    std::fs::write(&out_path, html).map_err(|e| format!("write graph.html: {}", e))?;
    Ok(out_path)
}

// ── Data assembly ──────────────────────────────────────────

fn compute_cc_map(graph: &DependencyGraph, root: &Path) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for module in &graph.modules {
        let abs = root.join(&module.path);
        let content = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let fc = analyze_file(&content, module.language);
        let max_cc = fc.max_complexity().map(|f| f.complexity).unwrap_or(0);
        map.insert(module.path.clone(), max_cc);
    }
    map
}

#[cfg(feature = "vector")]
fn load_module_communities(project: &Project, graph: &DependencyGraph) -> HashMap<String, usize> {
    use std::collections::HashMap as Map;

    let conn = match crate::vector_index::db::open_meta_db(project) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };

    // chunk_id → community_id
    let chunk_communities = match crate::vector_index::cluster::load_communities(&conn) {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };
    if chunk_communities.is_empty() {
        return HashMap::new();
    }

    // chunk_id → file_path. Avoids an N+1 by pulling the lookup once.
    let mut chunk_files: HashMap<i64, String> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT id, file_path FROM chunks") {
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                chunk_files.insert(row.0, row.1);
            }
        }
    }

    // Per file: distribution of communities across its chunks.
    let mut per_file: Map<String, Map<usize, usize>> = Map::new();
    for (chunk_id, community) in chunk_communities {
        if let Some(file) = chunk_files.get(&chunk_id) {
            *per_file
                .entry(file.clone())
                .or_default()
                .entry(community)
                .or_insert(0) += 1;
        }
    }

    // Pick the dominant community per file, then map onto graph modules.
    let module_paths: std::collections::HashSet<&str> =
        graph.modules.iter().map(|m| m.path.as_str()).collect();
    let mut out = HashMap::new();
    for (file, dist) in per_file {
        let normalized = file.replace('\\', "/");
        if !module_paths.contains(normalized.as_str()) {
            continue;
        }
        if let Some((community, _count)) = dist.into_iter().max_by_key(|&(_, c)| c) {
            out.insert(normalized, community);
        }
    }
    out
}

#[cfg(not(feature = "vector"))]
fn load_module_communities(_project: &Project, _graph: &DependencyGraph) -> HashMap<String, usize> {
    HashMap::new()
}

fn module_label(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    stem.to_string()
}

fn build_nodes_json(
    graph: &DependencyGraph,
    coupling: &HashMap<String, (usize, usize)>,
    cc: &HashMap<String, usize>,
    community: &HashMap<String, usize>,
) -> String {
    let total = graph.modules.len();
    let visible: Vec<&ModuleNode> = if total > LARGE_GRAPH_THRESHOLD {
        graph
            .modules
            .iter()
            .filter(|m| {
                let (fan_in, fan_out) = coupling.get(&m.path).copied().unwrap_or((0, 0));
                let module_cc = cc.get(&m.path).copied().unwrap_or(0);
                fan_in > 1 || fan_out > 1 || module_cc > 5
            })
            .collect()
    } else {
        graph.modules.iter().collect()
    };

    let mut entries: Vec<String> = Vec::with_capacity(visible.len());
    for m in &visible {
        let (fan_in, fan_out) = coupling.get(&m.path).copied().unwrap_or((0, 0));
        let module_cc = cc.get(&m.path).copied().unwrap_or(0);
        let community_value = community
            .get(&m.path)
            .map(|c| c.to_string())
            .unwrap_or_else(|| "null".to_string());
        entries.push(format!(
            "{{\"id\":{},\"label\":{},\"layer\":{},\"language\":{},\"loc\":{},\"cc\":{},\"community\":{},\"fan_in\":{},\"fan_out\":{}}}",
            json_string(&m.path),
            json_string(&module_label(&m.path)),
            json_string(&m.layer.to_string()),
            json_string(&language_str(m.language)),
            m.loc,
            module_cc,
            community_value,
            fan_in,
            fan_out,
        ));
    }
    format!("[{}]", entries.join(","))
}

fn build_edges_json(
    graph: &DependencyGraph,
    visible_ids: &std::collections::HashSet<&str>,
) -> String {
    let mut entries: Vec<String> = Vec::new();
    for edge in &graph.edges {
        if edge.is_external {
            continue;
        }
        // Drop edges whose endpoints were filtered out of the node set —
        // otherwise Cytoscape errors with "nonexistent target/source".
        if !visible_ids.contains(edge.from.as_str()) {
            continue;
        }
        if !visible_ids.contains(edge.to.as_str()) {
            continue;
        }
        entries.push(format!(
            "{{\"source\":{},\"target\":{},\"weight\":1}}",
            json_string(&edge.from),
            json_string(&edge.to),
        ));
    }
    format!("[{}]", entries.join(","))
}

fn language_str(lang: Language) -> String {
    lang.to_string()
}

/// Minimal JSON string encoder — covers the chars we actually emit (no
/// control chars from valid file paths or labels).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── HTML rendering ─────────────────────────────────────────

fn render_html(
    project_name: &str,
    nodes_json: &str,
    edges_json: &str,
    total_nodes: usize,
    shown_nodes: usize,
) -> String {
    let header_note = if shown_nodes < total_nodes {
        format!("showing {}/{} nodes", shown_nodes, total_nodes)
    } else {
        format!("{} nodes", total_nodes)
    };

    let community_colors_json = format!(
        "[{}]",
        COMMUNITY_COLORS
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(",")
    );

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title} — graph</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; background: #0F1117; color: #E6E8EE; }}
  #app {{ display: grid; grid-template-columns: 280px 1fr 320px; height: 100vh; }}
  #sidebar, #panel {{ background: #161922; padding: 16px; box-sizing: border-box; overflow-y: auto; }}
  #cy {{ background: #0F1117; width: 100%; height: 100%; min-height: 400px; }}
  h1 {{ font-size: 14px; margin: 0 0 8px; color: #9DA5B4; text-transform: uppercase; letter-spacing: 0.08em; }}
  .meta {{ font-size: 12px; color: #6E7686; margin-bottom: 16px; }}
  label {{ display: block; font-size: 12px; margin-top: 12px; color: #9DA5B4; }}
  input[type=range], input[type=text] {{ width: 100%; box-sizing: border-box; background: #1F2330; color: #E6E8EE; border: 1px solid #2A2F40; border-radius: 4px; padding: 6px 8px; }}
  input[type=text] {{ font-size: 13px; }}
  .layer-toggle {{ display: flex; align-items: center; gap: 6px; margin: 4px 0; font-size: 13px; }}
  .layer-toggle .swatch {{ display: inline-block; width: 12px; height: 12px; border-radius: 50%; }}
  button {{ background: #2A2F40; color: #E6E8EE; border: 0; padding: 8px 12px; border-radius: 4px; cursor: pointer; margin-top: 12px; margin-right: 8px; font-size: 13px; }}
  button:hover {{ background: #3A4055; }}
  #panel pre {{ background: #0F1117; border: 1px solid #2A2F40; border-radius: 4px; padding: 8px; font-size: 12px; overflow-x: auto; }}
  .kv {{ display: grid; grid-template-columns: 100px 1fr; font-size: 13px; gap: 4px; margin-top: 8px; }}
  .kv span:first-child {{ color: #9DA5B4; }}
  .empty {{ color: #6E7686; font-style: italic; font-size: 13px; }}
</style>
</head>
<body>
<div id="app">
  <div id="sidebar">
    <h1>Graph</h1>
    <div class="meta">{project} · {header_note}</div>

    <label for="search">Search</label>
    <input id="search" type="text" placeholder="filename or path">

    <label for="cc-min">Min CC: <span id="cc-min-val">0</span></label>
    <input id="cc-min" type="range" min="0" max="20" value="0">

    <h1 style="margin-top: 18px;">Layers</h1>
    <div id="layers"></div>

    <button id="reset">Reset</button>
    <button id="export">Export SVG</button>
  </div>

  <div id="cy"></div>

  <div id="panel">
    <h1>Selected node</h1>
    <div id="detail"><div class="empty">Click any node to see details.</div></div>
  </div>
</div>

<script>{cytoscape}</script>
<script>
const NODES = {nodes};
const EDGES = {edges};
const LAYER_COLORS = {{
  "Controller": "{c_controller}",
  "Service":    "{c_service}",
  "Repository": "{c_repository}",
  "Model":      "{c_model}",
}};
const COMMUNITY_COLORS = {community_colors};
const DEFAULT_COLOR = "{c_default}";
const LAYERS = ["Controller", "Service", "Repository", "Model", "Other"];

function layerColor(layer) {{
  return LAYER_COLORS[layer] || DEFAULT_COLOR;
}}

function communityColor(c) {{
  if (c === null || c === undefined) return "transparent";
  return COMMUNITY_COLORS[c % COMMUNITY_COLORS.length];
}}

const cyElements = NODES.map(n => ({{
  data: {{
    ...n,
    layerColor: layerColor(n.layer),
    communityColor: communityColor(n.community),
  }},
}})).concat(
  EDGES.map(e => ({{ data: e }}))
);

const cy = cytoscape({{
  container: document.getElementById('cy'),
  elements: cyElements,
  style: [
    {{
      selector: 'node',
      style: {{
        'background-color': 'data(layerColor)',
        'border-width': 3,
        'border-color': 'data(communityColor)',
        'label': 'data(label)',
        'color': '#E6E8EE',
        'font-size': 10,
        'text-valign': 'bottom',
        'text-margin-y': 6,
        'width': 18,
        'height': 18,
      }}
    }},
    {{
      selector: 'edge',
      style: {{
        'line-color': '#2A2F40',
        'width': 1,
        'target-arrow-shape': 'triangle',
        'target-arrow-color': '#2A2F40',
        'curve-style': 'bezier',
        'opacity': 0.7,
      }}
    }},
    {{
      selector: 'node.highlight',
      style: {{
        'background-color': '#FFD66B',
        'color': '#FFD66B',
      }}
    }},
    {{
      selector: 'node.dim',
      style: {{ 'opacity': 0.15 }}
    }},
  ],
  layout: {{ name: 'cose', animate: false, idealEdgeLength: 80, nodeRepulsion: 8000 }},
  ready: function() {{ cy.fit(undefined, 40); }}
}});

// ── Layer toggle UI ───────────────────────────────────────
const layersDiv = document.getElementById('layers');
const layerState = {{}};
LAYERS.forEach(l => {{
  layerState[l] = true;
  const row = document.createElement('label');
  row.className = 'layer-toggle';
  const sw = document.createElement('span');
  sw.className = 'swatch';
  sw.style.background = LAYER_COLORS[l] || DEFAULT_COLOR;
  const cb = document.createElement('input');
  cb.type = 'checkbox';
  cb.checked = true;
  cb.dataset.layer = l;
  cb.addEventListener('change', e => {{
    layerState[e.target.dataset.layer] = e.target.checked;
    applyFilters();
  }});
  row.appendChild(cb);
  row.appendChild(sw);
  row.appendChild(document.createTextNode(l));
  layersDiv.appendChild(row);
}});

// ── Filters ───────────────────────────────────────────────
const ccMin = document.getElementById('cc-min');
const ccMinVal = document.getElementById('cc-min-val');
ccMin.addEventListener('input', () => {{
  ccMinVal.textContent = ccMin.value;
  applyFilters();
}});

const searchBox = document.getElementById('search');
searchBox.addEventListener('input', () => applyFilters());

function applyFilters() {{
  const minCc = parseInt(ccMin.value, 10);
  const term = searchBox.value.trim().toLowerCase();
  cy.nodes().forEach(n => {{
    const layer = n.data('layer') || 'Other';
    const layerKey = LAYERS.includes(layer) ? layer : 'Other';
    const layerOn = layerState[layerKey];
    const ccOk = (n.data('cc') || 0) >= minCc;
    const matches = !term
      || (n.data('id') || '').toLowerCase().includes(term)
      || (n.data('label') || '').toLowerCase().includes(term);
    if (layerOn && ccOk && matches) {{
      n.style('display', 'element');
      n.removeClass('dim');
      n.toggleClass('highlight', !!term && matches);
    }} else {{
      n.style('display', 'none');
    }}
  }});
}}

// ── Detail panel ──────────────────────────────────────────
const detail = document.getElementById('detail');

function renderDetail(node) {{
  const d = node.data();
  const callers = cy.edges(`[target = "${{d.id}}"]`).map(e => e.source().data('id'));
  const callees = cy.edges(`[source = "${{d.id}}"]`).map(e => e.target().data('id'));
  const top = arr => arr.slice(0, 3).map(p => `<li>${{escapeHtml(p)}}</li>`).join('');
  detail.innerHTML = `
    <div class="kv">
      <span>path</span><span>${{escapeHtml(d.id)}}</span>
      <span>layer</span><span>${{escapeHtml(d.layer || 'Unknown')}}</span>
      <span>language</span><span>${{escapeHtml(d.language || '?')}}</span>
      <span>LOC</span><span>${{d.loc || 0}}</span>
      <span>CC (max)</span><span>${{d.cc || 0}}</span>
      <span>community</span><span>${{d.community === null || d.community === undefined ? '—' : d.community}}</span>
      <span>fan_in</span><span>${{d.fan_in || 0}}</span>
      <span>fan_out</span><span>${{d.fan_out || 0}}</span>
    </div>
    <h1 style="margin-top: 14px;">Top callers</h1>
    <ul style="margin: 4px 0 0; padding-left: 18px;">${{top(callers) || '<li class=empty>none</li>'}}</ul>
    <h1 style="margin-top: 14px;">Top callees</h1>
    <ul style="margin: 4px 0 0; padding-left: 18px;">${{top(callees) || '<li class=empty>none</li>'}}</ul>
  `;
}}

cy.on('tap', 'node', evt => renderDetail(evt.target));

// ── Reset + Export ────────────────────────────────────────
document.getElementById('reset').addEventListener('click', () => {{
  ccMin.value = 0;
  ccMinVal.textContent = '0';
  searchBox.value = '';
  Object.keys(layerState).forEach(k => {{ layerState[k] = true; }});
  document.querySelectorAll('#layers input').forEach(cb => {{ cb.checked = true; }});
  applyFilters();
  cy.layout({{ name: 'cose', animate: false, ready: function() {{ cy.fit(undefined, 40); }} }}).run();
}});

document.getElementById('export').addEventListener('click', () => {{
  // Cytoscape png() returns a data URL; cheap SVG approximation builds an
  // <svg> with circles per node and lines per edge using current positions.
  const svgNS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(svgNS, 'svg');
  const ext = cy.extent();
  const w = ext.w, h = ext.h;
  svg.setAttribute('viewBox', `${{ext.x1}} ${{ext.y1}} ${{w}} ${{h}}`);
  svg.setAttribute('xmlns', svgNS);
  cy.edges().filter(e => e.visible()).forEach(e => {{
    const s = e.source().position(), t = e.target().position();
    const line = document.createElementNS(svgNS, 'line');
    line.setAttribute('x1', s.x); line.setAttribute('y1', s.y);
    line.setAttribute('x2', t.x); line.setAttribute('y2', t.y);
    line.setAttribute('stroke', '#2A2F40'); line.setAttribute('stroke-width', '0.5');
    svg.appendChild(line);
  }});
  cy.nodes().filter(n => n.visible()).forEach(n => {{
    const p = n.position();
    const c = document.createElementNS(svgNS, 'circle');
    c.setAttribute('cx', p.x); c.setAttribute('cy', p.y); c.setAttribute('r', 6);
    c.setAttribute('fill', n.data('layerColor'));
    c.setAttribute('stroke', n.data('communityColor'));
    c.setAttribute('stroke-width', 2);
    svg.appendChild(c);
  }});
  const xml = new XMLSerializer().serializeToString(svg);
  const blob = new Blob([xml], {{ type: 'image/svg+xml' }});
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url; a.download = 'graph.svg';
  a.click();
  URL.revokeObjectURL(url);
}});

function escapeHtml(s) {{
  return String(s).replace(/[&<>'"]/g, ch => ({{
    '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;'
  }})[ch]);
}}
</script>
</body>
</html>
"#,
        title = project_name,
        project = project_name,
        header_note = header_note,
        cytoscape = CYTOSCAPE_JS,
        nodes = nodes_json,
        edges = edges_json,
        c_controller = COLOR_CONTROLLER,
        c_service = COLOR_SERVICE,
        c_repository = COLOR_REPOSITORY,
        c_model = COLOR_MODEL,
        c_default = COLOR_UNKNOWN,
        community_colors = community_colors_json,
    )
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::graph::{DependencyGraph, ImportEdge, Language, ModuleNode};
    use std::collections::HashSet;

    fn module(path: &str, layer: ArchLayer, loc: usize) -> ModuleNode {
        ModuleNode {
            path: path.to_string(),
            language: Language::Rust,
            layer,
            loc,
            class_count: 0,
            function_count: 0,
            is_hub: false,
            has_framework_macros: false,
        }
    }

    fn graph_with(modules: Vec<ModuleNode>, edges: Vec<ImportEdge>) -> DependencyGraph {
        DependencyGraph {
            root_path: "/tmp".to_string(),
            primary_language: Language::Rust,
            modules,
            edges,
            external_deps: HashSet::new(),
        }
    }

    #[test]
    fn test_layer_color_palette() {
        assert_eq!(layer_color(ArchLayer::Controller), COLOR_CONTROLLER);
        assert_eq!(layer_color(ArchLayer::Service), COLOR_SERVICE);
        assert_eq!(layer_color(ArchLayer::Repository), COLOR_REPOSITORY);
        assert_eq!(layer_color(ArchLayer::Model), COLOR_MODEL);
        assert_eq!(layer_color(ArchLayer::Unknown), COLOR_UNKNOWN);
        assert_eq!(layer_color(ArchLayer::Utility), COLOR_UNKNOWN);
        assert_eq!(layer_color(ArchLayer::Test), COLOR_UNKNOWN);
    }

    #[test]
    fn test_community_color_cycles_at_six() {
        assert_eq!(community_color(0), COMMUNITY_COLORS[0]);
        assert_eq!(community_color(5), COMMUNITY_COLORS[5]);
        assert_eq!(community_color(6), COMMUNITY_COLORS[0]);
        assert_eq!(community_color(13), COMMUNITY_COLORS[1]);
    }

    #[test]
    fn test_module_label_strips_extension_and_dirs() {
        assert_eq!(module_label("src/auth.rs"), "auth");
        assert_eq!(module_label("crates/foo/bar/baz.rs"), "baz");
        assert_eq!(module_label("Makefile"), "Makefile");
    }

    #[test]
    fn test_json_string_escapes_quotes_and_backslashes() {
        assert_eq!(json_string("hello"), "\"hello\"");
        assert_eq!(json_string("he said \"hi\""), "\"he said \\\"hi\\\"\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string("line\nbreak"), "\"line\\nbreak\"");
    }

    #[test]
    fn test_build_nodes_json_basic_shape() {
        let modules = vec![
            module("src/auth.rs", ArchLayer::Service, 100),
            module("src/db.rs", ArchLayer::Repository, 50),
        ];
        let g = graph_with(modules, vec![]);
        let coupling = g.coupling_metrics();
        let cc = HashMap::new();
        let community = HashMap::new();

        let json = build_nodes_json(&g, &coupling, &cc, &community);
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));
        assert!(json.contains("\"id\":\"src/auth.rs\""));
        assert!(json.contains("\"layer\":\"Service\""));
        assert!(json.contains("\"layer\":\"Repository\""));
        assert!(json.contains("\"loc\":100"));
        assert!(json.contains("\"community\":null"));
    }

    #[test]
    fn test_build_nodes_json_includes_community_when_present() {
        let modules = vec![module("src/auth.rs", ArchLayer::Service, 10)];
        let g = graph_with(modules, vec![]);
        let coupling = g.coupling_metrics();
        let cc = HashMap::new();
        let mut community = HashMap::new();
        community.insert("src/auth.rs".to_string(), 3);

        let json = build_nodes_json(&g, &coupling, &cc, &community);
        assert!(json.contains("\"community\":3"));
    }

    #[test]
    fn test_build_edges_json_skips_external() {
        let edges = vec![
            ImportEdge {
                from: "a.rs".to_string(),
                to: "b.rs".to_string(),
                is_external: false,
            },
            ImportEdge {
                from: "a.rs".to_string(),
                to: "serde".to_string(),
                is_external: true,
            },
        ];
        let g = graph_with(vec![], edges);
        let visible: HashSet<&str> = ["a.rs", "b.rs"].into_iter().collect();
        let json = build_edges_json(&g, &visible);
        assert!(json.contains("\"source\":\"a.rs\""));
        assert!(json.contains("\"target\":\"b.rs\""));
        assert!(!json.contains("serde"));
    }

    #[test]
    fn test_build_edges_json_filters_dangling_edges() {
        // A → B → C; B was filtered out (large-graph simplification or
        // similar). Both edges A→B and B→C reference B and must be dropped.
        let edges = vec![
            ImportEdge {
                from: "a.rs".to_string(),
                to: "b.rs".to_string(),
                is_external: false,
            },
            ImportEdge {
                from: "b.rs".to_string(),
                to: "c.rs".to_string(),
                is_external: false,
            },
        ];
        let g = graph_with(vec![], edges);
        let visible: HashSet<&str> = ["a.rs", "c.rs"].into_iter().collect();
        let json = build_edges_json(&g, &visible);
        assert!(
            !json.contains("b.rs"),
            "edges touching the filtered node should be dropped, got {}",
            json
        );
        // No surviving edges from this case.
        assert_eq!(json, "[]");
    }

    #[test]
    fn test_simplification_threshold_drops_simple_nodes() {
        // Build a graph above the simplification threshold where most nodes
        // are uninteresting (no edges, low CC). Only the noisy ones should
        // survive.
        let mut modules: Vec<ModuleNode> = (0..LARGE_GRAPH_THRESHOLD + 5)
            .map(|i| module(&format!("m{}.rs", i), ArchLayer::Service, 1))
            .collect();
        // The first three are kept by the filter (force fan_out > 1 / cc > 5).
        modules[0].layer = ArchLayer::Service;
        let edges = vec![
            ImportEdge {
                from: "m0.rs".to_string(),
                to: "m1.rs".to_string(),
                is_external: false,
            },
            ImportEdge {
                from: "m0.rs".to_string(),
                to: "m2.rs".to_string(),
                is_external: false,
            },
        ];
        let g = graph_with(modules, edges);
        let coupling = g.coupling_metrics();
        let mut cc = HashMap::new();
        cc.insert("m3.rs".to_string(), 10); // CC above threshold keeps this one too.
        let community = HashMap::new();

        let json = build_nodes_json(&g, &coupling, &cc, &community);
        let count = json.matches("\"id\":").count();
        assert!(
            count < LARGE_GRAPH_THRESHOLD,
            "simplification should drop simple nodes; got {} kept",
            count
        );
        // m0 has fan_out=2 → keep. m3 has cc=10 → keep.
        assert!(json.contains("\"id\":\"m0.rs\""));
        assert!(json.contains("\"id\":\"m3.rs\""));
    }

    #[test]
    fn test_render_html_self_contained() {
        let html = render_html("demo", "[]", "[]", 0, 0);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("cytoscape"), "must inline cytoscape js");
        // Should never reference an external CDN.
        assert!(
            !html.contains("https://cdn") && !html.contains("https://unpkg"),
            "graph.html must be self-contained (no CDN refs)"
        );
    }

    #[test]
    fn test_render_html_shows_truncation_note() {
        let html = render_html("demo", "[]", "[]", 1000, 200);
        assert!(html.contains("showing 200/1000 nodes"));
    }

    #[test]
    fn test_render_html_size_under_600kb() {
        // Regression guard: the embedded Cytoscape + UI must keep us
        // comfortably under the 600KB target the spec calls for.
        let html = render_html("demo", "[]", "[]", 0, 0);
        assert!(
            html.len() < 600_000,
            "graph.html grew past 600KB ({}b)",
            html.len()
        );
    }
}
