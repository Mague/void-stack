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

/// Build the self-contained `graph.html` content for `project` as a string
/// (no disk write). Used by the in-app viewer (embedded in an iframe) and by
/// [`generate_graph_html`].
pub fn build_graph_html(project: &Project) -> Result<String, String> {
    let root = Path::new(&project.path);
    let graph = build_graph(root).ok_or_else(|| {
        "No source files found to build a dependency graph (the project looks \
         empty or uses an unsupported language: Python, JS/TS, Go, Dart or Rust)."
            .to_string()
    })?;

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

    Ok(render_html(
        &project.name,
        &nodes_json,
        &edges_json,
        total_nodes,
        shown_nodes,
    ))
}

/// Generate `graph.html` for `project`, return the absolute path it was written to.
pub fn generate_graph_html(project: &Project) -> Result<PathBuf, String> {
    let html = build_graph_html(project)?;
    let root = Path::new(&project.path);
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

/// Minimal HTML-attribute/text escape for the project name in the chrome.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Polished single-file viewer chrome. Tokens (`__NODES__`, `__EDGES__`,
/// `__LAYER_COLORS__`, `__COMMUNITY_COLORS__`, `__DEFAULT_COLOR__`,
/// `__PROJECT__`, `__TITLE__`, `__HEADER_NOTE__`, `/*__CYTOSCAPE__*/`) are
/// substituted in [`render_html`].
const GRAPH_TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>__TITLE__ — graph</title>
<style>
:root{--bg:#0B0E14;--surface:#10141C;--raised:#151A24;--hover:#1A2030;--line:rgba(255,255,255,.08);--line2:rgba(255,255,255,.14);--text:#E7EBF3;--text2:#8B94A7;--text3:#5A6375;--accent:#3BC9FF;--violet:#A875F5}
*{box-sizing:border-box}
html,body{margin:0;height:100%;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;background:var(--bg);color:var(--text);font-size:13px}
#wrap{display:flex;flex-direction:column;height:100vh}
#bar{display:flex;align-items:center;gap:10px;height:48px;padding:0 14px;border-bottom:1px solid var(--line);flex-shrink:0}
#bar .brand{font-weight:600;letter-spacing:.02em}
#bar .count{color:var(--text3);font-size:12px}
#bar .sp{flex:1}
#bar input[type=text],#bar select{background:var(--surface);border:1px solid var(--line);border-radius:8px;color:var(--text);padding:6px 10px;font-size:12.5px;outline:none}
#bar input[type=text]:focus,#bar select:focus{border-color:var(--line2)}
.btn{background:var(--surface);border:1px solid var(--line);border-radius:8px;color:var(--text2);padding:6px 10px;font-size:12.5px;cursor:pointer}
.btn:hover{border-color:var(--line2);color:var(--text)}
#body{display:grid;grid-template-columns:230px 1fr 300px;flex:1;min-height:0;position:relative}
#left,#right{background:var(--surface);padding:14px;overflow-y:auto}
#left{border-right:1px solid var(--line)}
#right{border-left:1px solid var(--line)}
#cy{background:var(--bg);min-height:0}
h2{font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:var(--text3);margin:0 0 8px}
.leg{display:flex;align-items:center;gap:8px;padding:5px 0;font-size:12.5px;cursor:pointer;color:var(--text2)}
.leg input{accent-color:var(--accent)}
.leg .sw{width:11px;height:11px;border-radius:50%}
.leg.off{opacity:.4}
label.rng{display:block;color:var(--text3);font-size:12px;margin:14px 0 6px}
input[type=range]{width:100%;accent-color:var(--accent)}
.kv{display:grid;grid-template-columns:84px 1fr;gap:5px 8px;font-size:12.5px;margin-top:6px}
.kv b{color:var(--text3);font-weight:400}
.kv span{color:var(--text);word-break:break-all}
.empty{color:var(--text3);font-style:italic}
#right ul{margin:6px 0 0;padding-left:16px;font-size:12px;color:var(--text2)}
#right li{margin:2px 0;word-break:break-all}
#right ul.files li{cursor:pointer}
#right ul.files li:hover{color:var(--accent)}
.zoomctl{position:absolute;right:316px;bottom:16px;display:flex;flex-direction:column;gap:4px;z-index:5}
.zoomctl .btn{width:30px;height:30px;font-size:16px;line-height:1;padding:0;text-align:center}
#loading{position:absolute;inset:0 300px 0 230px;display:flex;align-items:center;justify-content:center;color:var(--text3);font-size:13px;pointer-events:none;background:rgba(11,14,20,.6)}
</style>
</head>
<body>
<div id="wrap">
  <div id="bar">
    <span class="brand">__PROJECT__</span>
    <span class="count" id="count">__HEADER_NOTE__</span>
    <span class="sp"></span>
    <input id="search" type="text" placeholder="search file…">
    <select id="colorby"><option value="layer">color: layer</option><option value="community">color: community</option></select>
    <select id="layout"><option value="cose">force</option><option value="concentric">concentric</option><option value="breadthfirst">hierarchical</option><option value="grid">grid</option></select>
    <button class="btn" id="fit">Fit</button>
    <button class="btn" id="export">SVG</button>
  </div>
  <div id="body">
    <div id="left">
      <h2>Layers</h2>
      <div id="layers"></div>
      <label class="rng" for="cc">Min CC: <span id="ccv">0</span></label>
      <input id="cc" type="range" min="0" max="20" value="0">
    </div>
    <div id="cy"></div>
    <div id="loading">Computing layout…</div>
    <div class="zoomctl"><button class="btn" id="zin">+</button><button class="btn" id="zout">−</button></div>
    <div id="right">
      <h2>Node</h2>
      <div id="detail"><div class="empty">Click a node for details.</div></div>
    </div>
  </div>
</div>
<script>/*__CYTOSCAPE__*/</script>
<script>
const NODES=__NODES__;
const EDGES=__EDGES__;
const LAYER_COLORS=__LAYER_COLORS__;
const COMMUNITY_COLORS=__COMMUNITY_COLORS__;
const DEFAULT_COLOR="__DEFAULT_COLOR__";
const LAYERS=["Controller","Service","Repository","Model","Other"];
function layerColor(l){return LAYER_COLORS[l]||DEFAULT_COLOR}
function communityColor(c){if(c==null)return DEFAULT_COLOR;return COMMUNITY_COLORS[c%COMMUNITY_COLORS.length]}
const els=NODES.map(n=>({data:{...n,importance:(n.fan_in||0)+(n.fan_out||0),layerColor:layerColor(n.layer),communityColor:communityColor(n.community)}})).concat(EDGES.map(e=>({data:e})));
document.getElementById('count').textContent=document.getElementById('count').textContent+' · '+EDGES.length+' aristas';
const cy=cytoscape({
  container:document.getElementById('cy'),
  elements:els,
  wheelSensitivity:0.2,
  style:[
    {selector:'node',style:{'background-color':'data(layerColor)','border-width':2,'border-color':'data(communityColor)','label':'data(label)','color':'#8B94A7','font-size':9,'text-valign':'bottom','text-margin-y':4,'width':'mapData(importance,0,30,16,48)','height':'mapData(importance,0,30,16,48)','min-zoomed-font-size':7}},
    {selector:'edge',style:{'line-color':'rgba(255,255,255,0.08)','width':1,'target-arrow-shape':'triangle','target-arrow-color':'rgba(255,255,255,0.08)','arrow-scale':0.7,'curve-style':'bezier'}},
    {selector:'node.sel',style:{'border-color':'#3BC9FF','border-width':3,'color':'#E7EBF3','font-size':11,'z-index':10}},
    {selector:'node.nb',style:{'color':'#E7EBF3'}},
    {selector:'edge.nb',style:{'line-color':'#3BC9FF','target-arrow-color':'#3BC9FF','width':2,'z-index':9}},
    {selector:'.faded',style:{'opacity':0.08}},
    {selector:'node.hidden',style:{'display':'none'}}
  ],
  layout:{name:'preset'}
});
const loading=document.getElementById('loading');
function runLayout(name){loading.style.display='flex';const opt={name,animate:false};if(name==='cose'){opt.idealEdgeLength=70;opt.nodeRepulsion=9000;opt.nodeOverlap=12}if(name==='concentric'){opt.concentric=n=>n.data('importance');opt.levelWidth=()=>4}const lo=cy.layout(opt);let done=false;const finish=()=>{if(done)return;done=true;loading.style.display='none';cy.fit(undefined,40)};lo.one('layoutstop',finish);setTimeout(finish,6000);lo.run()}
runLayout('cose');
function openFile(file,line){try{window.parent.postMessage({source:'void-graph',type:'open',file:file,line:line||1},'*')}catch(e){}}
let sticky=null;
function focusNode(node){const nb=node.closedNeighborhood();cy.elements().addClass('faded');nb.removeClass('faded');node.addClass('sel');nb.nodes().addClass('nb');nb.connectedEdges().addClass('nb')}
function clearFocus(){cy.elements().removeClass('faded nb sel')}
cy.on('mouseover','node',e=>{if(!sticky)focusNode(e.target)});
cy.on('mouseout','node',()=>{if(!sticky)clearFocus()});
cy.on('tap','node',e=>{sticky=e.target;clearFocus();focusNode(e.target);renderDetail(e.target)});
cy.on('tap',e=>{if(e.target===cy){sticky=null;clearFocus()}});
document.getElementById('colorby').addEventListener('change',ev=>{const m=ev.target.value;cy.batch(()=>cy.nodes().forEach(n=>n.style('background-color',m==='community'?n.data('communityColor'):n.data('layerColor'))))});
document.getElementById('layout').addEventListener('change',ev=>runLayout(ev.target.value));
const layersDiv=document.getElementById('layers'),layerOn={};
LAYERS.forEach(l=>{layerOn[l]=true;const row=document.createElement('label');row.className='leg';const cb=document.createElement('input');cb.type='checkbox';cb.checked=true;const sw=document.createElement('span');sw.className='sw';sw.style.background=layerColor(l);cb.addEventListener('change',()=>{layerOn[l]=cb.checked;row.classList.toggle('off',!cb.checked);applyFilters()});row.appendChild(cb);row.appendChild(sw);row.appendChild(document.createTextNode(' '+l));layersDiv.appendChild(row)});
const cc=document.getElementById('cc'),ccv=document.getElementById('ccv'),search=document.getElementById('search');
cc.addEventListener('input',()=>{ccv.textContent=cc.value;applyFilters()});
search.addEventListener('input',applyFilters);
function applyFilters(){const min=+cc.value;const term=search.value.trim().toLowerCase();cy.batch(()=>{cy.nodes().forEach(n=>{const l=n.data('layer');const lk=LAYERS.includes(l)?l:'Other';const ok=layerOn[lk]&&(n.data('cc')||0)>=min&&(!term||(n.data('id')||'').toLowerCase().includes(term)||(n.data('label')||'').toLowerCase().includes(term));n.toggleClass('hidden',!ok)})})}
const detail=document.getElementById('detail');
function renderDetail(node){const d=node.data();const callers=cy.edges('[target = "'+d.id+'"]').map(e=>e.source().data('id'));const callees=cy.edges('[source = "'+d.id+'"]').map(e=>e.target().data('id'));const li=a=>a.slice(0,6).map(p=>'<li data-open="'+esc(p)+'" title="Open in editor">'+esc(p)+'</li>').join('')||'<li class=empty>none</li>';detail.innerHTML='<div class=kv><b>file</b><span>'+esc(d.id)+'</span><b>layer</b><span>'+esc(d.layer||'?')+'</span><b>language</b><span>'+esc(d.language||'?')+'</span><b>LOC</b><span>'+(d.loc||0)+'</span><b>CC max</b><span>'+(d.cc||0)+'</span><b>community</b><span>'+(d.community==null?'—':d.community)+'</span><b>fan-in</b><span>'+(d.fan_in||0)+'</span><b>fan-out</b><span>'+(d.fan_out||0)+'</span></div><button class="btn" id="openbtn" style="margin-top:10px">Open in editor</button><h2 style="margin-top:14px">Imported by ('+callers.length+')</h2><ul class=files>'+li(callers)+'</ul><h2 style="margin-top:14px">Imports ('+callees.length+')</h2><ul class=files>'+li(callees)+'</ul>';const ob=detail.querySelector('#openbtn');if(ob)ob.addEventListener('click',()=>openFile(d.id));detail.querySelectorAll('li[data-open]').forEach(el=>el.addEventListener('click',()=>openFile(el.getAttribute('data-open'))))}
document.getElementById('fit').onclick=()=>cy.animate({fit:{padding:40}},{duration:200});
document.getElementById('zin').onclick=()=>cy.zoom({level:cy.zoom()*1.3,renderedPosition:{x:cy.width()/2,y:cy.height()/2}});
document.getElementById('zout').onclick=()=>cy.zoom({level:cy.zoom()/1.3,renderedPosition:{x:cy.width()/2,y:cy.height()/2}});
document.getElementById('export').onclick=()=>{const ns='http://www.w3.org/2000/svg';const svg=document.createElementNS(ns,'svg');const ext=cy.extent();svg.setAttribute('viewBox',ext.x1+' '+ext.y1+' '+ext.w+' '+ext.h);svg.setAttribute('xmlns',ns);cy.edges(':visible').forEach(e=>{const s=e.source().position(),t=e.target().position();const l=document.createElementNS(ns,'line');l.setAttribute('x1',s.x);l.setAttribute('y1',s.y);l.setAttribute('x2',t.x);l.setAttribute('y2',t.y);l.setAttribute('stroke','#2A2F40');l.setAttribute('stroke-width','0.5');svg.appendChild(l)});cy.nodes(':visible').forEach(n=>{const p=n.position();const c=document.createElementNS(ns,'circle');c.setAttribute('cx',p.x);c.setAttribute('cy',p.y);c.setAttribute('r',6);c.setAttribute('fill',n.data('layerColor'));c.setAttribute('stroke',n.data('communityColor'));c.setAttribute('stroke-width',2);svg.appendChild(c)});const blob=new Blob([new XMLSerializer().serializeToString(svg)],{type:'image/svg+xml'});const url=URL.createObjectURL(blob);const a=document.createElement('a');a.href=url;a.download='graph.svg';a.click();URL.revokeObjectURL(url)};
function esc(s){return String(s).replace(/[&<>'"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;',"'":'&#39;','"':'&quot;'}[c]))}
</script>
</body>
</html>
"##;

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
    let layer_colors_json = format!(
        "{{\"Controller\":\"{}\",\"Service\":\"{}\",\"Repository\":\"{}\",\"Model\":\"{}\"}}",
        COLOR_CONTROLLER, COLOR_SERVICE, COLOR_REPOSITORY, COLOR_MODEL
    );

    // Template uses unique tokens + .replace() instead of format! brace
    // escaping — far less error-prone for a big embedded JS/CSS blob. The
    // huge Cytoscape blob is substituted last so earlier passes stay cheap.
    GRAPH_TEMPLATE
        .replace("__TITLE__", &html_escape(project_name))
        .replace("__PROJECT__", &html_escape(project_name))
        .replace("__HEADER_NOTE__", &html_escape(&header_note))
        .replace("__NODES__", nodes_json)
        .replace("__EDGES__", edges_json)
        .replace("__LAYER_COLORS__", &layer_colors_json)
        .replace("__COMMUNITY_COLORS__", &community_colors_json)
        .replace("__DEFAULT_COLOR__", COLOR_UNKNOWN)
        .replace("/*__CYTOSCAPE__*/", CYTOSCAPE_JS)
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
