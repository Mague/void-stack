//! `list_entrypoints`: every way into a project — HTTP routes, gRPC RPCs,
//! MCP tools, CLI subcommands, UI routes and background jobs — one line
//! each, extracted from the source with regexes (REST and gRPC reuse the
//! contract extractors).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::walk::{SourceFile, language, walk};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    Rest,
    Grpc,
    Mcp,
    Cli,
    Routes,
    Jobs,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Rest => "rest",
            EntryKind::Grpc => "grpc",
            EntryKind::Mcp => "mcp",
            EntryKind::Cli => "cli",
            EntryKind::Routes => "routes",
            EntryKind::Jobs => "jobs",
        }
    }

    pub fn parse(s: &str) -> Option<EntryKind> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "rest" | "http" | "api" => EntryKind::Rest,
            "grpc" | "rpc" => EntryKind::Grpc,
            "mcp" | "tools" => EntryKind::Mcp,
            "cli" | "commands" => EntryKind::Cli,
            "routes" | "ui" | "pages" | "screens" => EntryKind::Routes,
            "jobs" | "cron" | "workers" => EntryKind::Jobs,
            _ => return None,
        })
    }

    pub const ALL: [EntryKind; 6] = [
        EntryKind::Rest,
        EntryKind::Grpc,
        EntryKind::Mcp,
        EntryKind::Cli,
        EntryKind::Routes,
        EntryKind::Jobs,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntryPoint {
    pub kind: EntryKind,
    pub name: String,
    pub file: String,
    pub line: usize,
    pub description: String,
}

/// Largest source file scanned (bundles, generated code are skipped).
const MAX_SCAN_BYTES: u64 = 512 * 1024;

/// Extract entry points of the requested kinds (all when `kinds` is
/// empty) from `project`.
pub fn entrypoints(project: &crate::model::Project, kinds: &[EntryKind]) -> Vec<EntryPoint> {
    let root_buf = std::path::PathBuf::from(crate::runner::local::strip_win_prefix(&project.path));
    let root: &Path = &root_buf;
    let want = |k: EntryKind| kinds.is_empty() || kinds.contains(&k);
    let files = walk(root);
    let mut out = Vec::new();

    let scan_text = want(EntryKind::Mcp)
        || want(EntryKind::Cli)
        || want(EntryKind::Routes)
        || want(EntryKind::Jobs);
    if scan_text {
        for f in files.iter().filter(|f| scannable(f)) {
            let Some(text) = crate::fs_util::read_source(&root.join(&f.rel)) else {
                continue;
            };
            extract_text(&f.rel, &text, &want, &mut out);
        }
        if want(EntryKind::Routes) {
            next_pages(&files, &mut out);
        }
    }
    #[cfg(feature = "vector")]
    if want(EntryKind::Rest) || want(EntryKind::Grpc) {
        contracts(project, &want, &mut out);
    }
    out.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
    });
    out.dedup_by(|a, b| a.kind == b.kind && a.name == b.name && a.file == b.file);
    out
}

fn scannable(f: &SourceFile) -> bool {
    let l = f.rel.to_ascii_lowercase();
    language(&f.rel).is_some()
        && f.size <= MAX_SCAN_BYTES
        && !l.contains(".min.")
        && !l.ends_with(".d.ts")
        && !l.ends_with(".pb.go")
        && !l.ends_with(".pb.dart")
        && !l.ends_with(".pbgrpc.dart")
        && !l.ends_with(".g.dart")
        && !l.ends_with(".freezed.dart")
}

#[cfg(feature = "vector")]
fn contracts(
    project: &crate::model::Project,
    want: &impl Fn(EntryKind) -> bool,
    out: &mut Vec<EntryPoint>,
) {
    use crate::vector_index::{ContractKind, ContractRole};
    for c in crate::vector_index::project_contracts(project) {
        if c.role != ContractRole::Producer {
            continue;
        }
        let kind = match c.kind {
            ContractKind::Rest => EntryKind::Rest,
            ContractKind::Grpc => EntryKind::Grpc,
            ContractKind::GrpcProtoHash => continue,
        };
        if !want(kind) {
            continue;
        }
        out.push(EntryPoint {
            kind,
            name: c.key,
            file: c.file,
            line: c.line,
            description: c.detail,
        });
    }
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("valid regex"));
    };
}

// MCP tools
re!(RS_TOOL_ATTR, r#"^\s*#\[tool\("#);
re!(
    RS_FN_NAME,
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)"
);
re!(DESCRIPTION, r#"description\s*=\s*"((?:[^"\\]|\\.)*)""#);
re!(
    TS_TOOL,
    r#"\.(?:tool|registerTool)\(\s*["'`]([\w.\-]+)["'`]\s*,\s*(?:\{[^}]*?description\s*:\s*)?(?:["'`]([^"'`]*)["'`])?"#
);
re!(PY_MCP_TOOL, r"^\s*@(?:\w+\.)?tool\(");
re!(PY_DEF, r"^\s*(?:async\s+)?def\s+(\w+)");
re!(
    DART_TOOL_NAME,
    r#"^\s*(?:name|toolName)\s*:\s*'([a-z][\w]*)'"#
);
re!(DART_DESCRIPTION, r#"description\s*:\s*'([^']*)'"#);
// CLI
re!(RS_SUBCOMMAND_DERIVE, r"#\[derive\([^)]*Subcommand");
re!(RS_ENUM, r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)");
re!(RS_VARIANT, r"^\s{4}([A-Z]\w*)\s*(?:\{|\(|,|$)");
re!(RS_COMMAND_NAME, r#"#\[command\([^)]*name\s*=\s*"([^"]+)""#);
re!(
    PY_ADD_PARSER,
    r#"add_parser\(\s*["']([\w\-]+)["'](?:[^)]*help\s*=\s*["']([^"']*)["'])?"#
);
re!(PY_CLICK, r"^\s*@(?:\w+\.)?command\(");
re!(GO_COBRA_USE, r#"Use:\s*"([\w\-]+)"#);
re!(GO_COBRA_SHORT, r#"Short:\s*"([^"]*)""#);
re!(JS_COMMANDER, r#"\.command\(\s*['"]([\w\-]+)"#);
re!(JS_DESCRIPTION, r#"\.description\(\s*['"`]([^'"`]*)"#);
// UI routes
re!(GO_ROUTE, r#"GoRoute\("#);
re!(PATH_PROP, r#"path\s*:\s*['"]([^'"]+)['"]"#);
re!(NAME_PROP, r#"name\s*:\s*['"]([^'"]+)['"]"#);
re!(REACT_ROUTE, r#"<Route\s[^>]*path\s*=\s*["']([^"']+)["']"#);
re!(
    ROUTE_OBJ,
    r#"\{\s*path\s*:\s*['"]([^'"]+)['"]\s*,\s*(?:element|component|name|redirect|children)"#
);
re!(
    FLUTTER_NAMED,
    r#"^\s*['"](/[\w/\-:]*)['"]\s*:\s*\(\s*(?:context|_|ctx)"#
);
// Jobs
re!(
    JOB,
    r#"(cron::|Cron(?:Job)?\(|cron\.schedule|schedule\.every|@Scheduled|setInterval\(|Timer\.periodic|tokio::time::interval|add_job\(|@(?:shared_task|app\.task|celery\.task)|BackgroundScheduler|AsyncIOScheduler|recommended_watcher|chokidar\.watch|fs\.watch\(|new Worker\(|@nestjs/schedule|@Cron\()"#
);

fn line_of(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].matches('\n').count() + 1
}

/// Doc comment right above line `idx` (Rust `///`, Python/TS `#`/`//`).
fn doc_above(lines: &[&str], idx: usize) -> String {
    let mut docs = Vec::new();
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let t = lines[i].trim();
        if let Some(d) = t.strip_prefix("///").or_else(|| t.strip_prefix("//!")) {
            docs.push(d.trim());
        } else if t.starts_with("#[") || t.is_empty() && docs.is_empty() {
            continue;
        } else {
            break;
        }
    }
    docs.reverse();
    clip(&docs.join(" "), 120)
}

fn clip(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        return s;
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn push(
    out: &mut Vec<EntryPoint>,
    kind: EntryKind,
    name: impl Into<String>,
    file: &str,
    line: usize,
    description: impl Into<String>,
) {
    out.push(EntryPoint {
        kind,
        name: name.into(),
        file: file.to_string(),
        line,
        description: clip(&description.into(), 120),
    });
}

fn extract_text(
    rel: &str,
    text: &str,
    want: &impl Fn(EntryKind) -> bool,
    out: &mut Vec<EntryPoint>,
) {
    let lang = language(rel).unwrap_or("");
    let lines: Vec<&str> = text.lines().collect();

    if want(EntryKind::Mcp) {
        match lang {
            "Rust" if text.contains("#[tool(") => {
                for (i, l) in lines.iter().enumerate() {
                    if !RS_TOOL_ATTR.is_match(l) {
                        continue;
                    }
                    // The attribute may span lines; the fn follows within ~12.
                    let window = lines[i..(i + 12).min(lines.len())].join("\n");
                    let desc = DESCRIPTION
                        .captures(&window)
                        .map(|c| c[1].replace("\\\"", "\""))
                        .unwrap_or_default();
                    if let Some((j, name)) = lines
                        .iter()
                        .enumerate()
                        .skip(i + 1)
                        .take(12)
                        .find_map(|(j, l)| RS_FN_NAME.captures(l).map(|c| (j, c[1].to_string())))
                    {
                        push(out, EntryKind::Mcp, name, rel, j + 1, desc);
                    }
                }
            }
            "TypeScript" | "JavaScript"
                if text.contains(".tool(") || text.contains(".registerTool(") =>
            {
                for c in TS_TOOL.captures_iter(text) {
                    let m = c.get(0).map(|m| m.start()).unwrap_or(0);
                    push(
                        out,
                        EntryKind::Mcp,
                        &c[1],
                        rel,
                        line_of(text, m),
                        c.get(2).map(|d| d.as_str()).unwrap_or(""),
                    );
                }
            }
            "Python" if text.contains("tool(") => {
                for (i, l) in lines.iter().enumerate() {
                    if !PY_MCP_TOOL.is_match(l) {
                        continue;
                    }
                    if let Some((j, name)) = lines
                        .iter()
                        .enumerate()
                        .skip(i + 1)
                        .take(4)
                        .find_map(|(j, l)| PY_DEF.captures(l).map(|c| (j, c[1].to_string())))
                    {
                        let doc = lines
                            .get(j + 1)
                            .map(|d| d.trim().trim_matches('"').trim_matches('\'').to_string())
                            .unwrap_or_default();
                        push(out, EntryKind::Mcp, name, rel, j + 1, doc);
                    }
                }
            }
            "Dart"
                if (text.contains("ToolRegistry") || rel.to_ascii_lowercase().contains("tool"))
                    && text.contains("description") =>
            {
                for (i, l) in lines.iter().enumerate() {
                    if let Some(c) = DART_TOOL_NAME.captures(l) {
                        let window = lines[i..(i + 4).min(lines.len())].join(" ");
                        let desc = DART_DESCRIPTION
                            .captures(&window)
                            .map(|d| d[1].to_string())
                            .unwrap_or_default();
                        if !desc.is_empty() {
                            push(out, EntryKind::Mcp, &c[1], rel, i + 1, desc);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if want(EntryKind::Cli) {
        match lang {
            "Rust" if text.contains("Subcommand") => {
                let mut i = 0;
                while i < lines.len() {
                    if RS_SUBCOMMAND_DERIVE.is_match(lines[i]) {
                        // Find the enum and walk its top-level variants.
                        let Some(start) =
                            (i..(i + 6).min(lines.len())).find(|&k| RS_ENUM.is_match(lines[k]))
                        else {
                            i += 1;
                            continue;
                        };
                        let mut depth = 0i32;
                        let mut k = start;
                        let mut pending_name: Option<String> = None;
                        while k < lines.len() {
                            let l = lines[k];
                            if depth == 1 {
                                if let Some(c) = RS_COMMAND_NAME.captures(l) {
                                    pending_name = Some(c[1].to_string());
                                } else if let Some(c) = RS_VARIANT.captures(l) {
                                    let name = pending_name.take().unwrap_or_else(|| kebab(&c[1]));
                                    push(
                                        out,
                                        EntryKind::Cli,
                                        name,
                                        rel,
                                        k + 1,
                                        doc_above(&lines, k),
                                    );
                                }
                            }
                            depth += l.matches('{').count() as i32 - l.matches('}').count() as i32;
                            if depth <= 0 && k > start {
                                break;
                            }
                            k += 1;
                        }
                        i = k;
                    }
                    i += 1;
                }
            }
            "Python" => {
                for c in PY_ADD_PARSER.captures_iter(text) {
                    let m = c.get(0).map(|m| m.start()).unwrap_or(0);
                    push(
                        out,
                        EntryKind::Cli,
                        &c[1],
                        rel,
                        line_of(text, m),
                        c.get(2).map(|d| d.as_str()).unwrap_or(""),
                    );
                }
                if text.contains("import click")
                    || text.contains("from click")
                    || text.contains("typer")
                {
                    for (i, l) in lines.iter().enumerate() {
                        if PY_CLICK.is_match(l)
                            && let Some((j, name)) = lines
                                .iter()
                                .enumerate()
                                .skip(i + 1)
                                .take(6)
                                .find_map(|(j, l)| {
                                    PY_DEF.captures(l).map(|c| (j, c[1].to_string()))
                                })
                        {
                            push(out, EntryKind::Cli, name.replace('_', "-"), rel, j + 1, "");
                        }
                    }
                }
            }
            "Go" if text.contains("cobra.Command") => {
                for c in GO_COBRA_USE.captures_iter(text) {
                    let m = c.get(0).map(|m| m.start()).unwrap_or(0);
                    let tail = &text[m..(m + 400).min(text.len())];
                    let short = GO_COBRA_SHORT
                        .captures(tail)
                        .map(|s| s[1].to_string())
                        .unwrap_or_default();
                    push(out, EntryKind::Cli, &c[1], rel, line_of(text, m), short);
                }
            }
            "TypeScript" | "JavaScript" if text.contains("commander") || text.contains("yargs") => {
                for c in JS_COMMANDER.captures_iter(text) {
                    let m = c.get(0).map(|m| m.start()).unwrap_or(0);
                    let tail = &text[m..(m + 300).min(text.len())];
                    let desc = JS_DESCRIPTION
                        .captures(tail)
                        .map(|s| s[1].to_string())
                        .unwrap_or_default();
                    push(out, EntryKind::Cli, &c[1], rel, line_of(text, m), desc);
                }
            }
            _ => {}
        }
    }

    if want(EntryKind::Routes) {
        match lang {
            "Dart" => {
                if text.contains("GoRoute(") {
                    for m in GO_ROUTE.find_iter(text) {
                        let tail = &text[m.start()..(m.start() + 300).min(text.len())];
                        if let Some(p) = PATH_PROP.captures(tail) {
                            let name = NAME_PROP
                                .captures(tail)
                                .map(|n| n[1].to_string())
                                .unwrap_or_default();
                            push(
                                out,
                                EntryKind::Routes,
                                &p[1],
                                rel,
                                line_of(text, m.start()),
                                name,
                            );
                        }
                    }
                }
                for (i, l) in lines.iter().enumerate() {
                    if let Some(c) = FLUTTER_NAMED.captures(l) {
                        push(out, EntryKind::Routes, &c[1], rel, i + 1, "");
                    }
                }
            }
            "TypeScript" | "JavaScript" | "Vue" | "Svelte" => {
                for c in REACT_ROUTE.captures_iter(text) {
                    let m = c.get(0).map(|m| m.start()).unwrap_or(0);
                    push(out, EntryKind::Routes, &c[1], rel, line_of(text, m), "");
                }
                if rel.to_ascii_lowercase().contains("rout") {
                    for c in ROUTE_OBJ.captures_iter(text) {
                        let m = c.get(0).map(|m| m.start()).unwrap_or(0);
                        push(out, EntryKind::Routes, &c[1], rel, line_of(text, m), "");
                    }
                }
            }
            _ => {}
        }
    }

    if want(EntryKind::Jobs) {
        let mut seen_lines = 0;
        for (i, l) in lines.iter().enumerate() {
            let t = l.trim_start();
            if t.starts_with("//")
                || t.starts_with('#') && !t.starts_with("#[")
                || t.starts_with('*')
            {
                continue;
            }
            if let Some(m) = JOB.find(l) {
                seen_lines += 1;
                if seen_lines > 6 {
                    break;
                }
                let name = m.as_str().trim_end_matches('(').to_string();
                push(out, EntryKind::Jobs, name, rel, i + 1, l.trim());
            }
        }
    }
}

/// Next.js file-system routes: `app/**/page.tsx` and `pages/**`.
fn next_pages(files: &[SourceFile], out: &mut Vec<EntryPoint>) {
    for f in files {
        let l = f.rel.replace('\\', "/");
        let Some((before, after)) = l.rsplit_once("app/").or_else(|| l.rsplit_once("pages/"))
        else {
            continue;
        };
        if before.contains("node_modules") {
            continue;
        }
        let is_app_page = l.contains("app/")
            && after
                .rsplit('/')
                .next()
                .is_some_and(|n| n.starts_with("page."));
        let is_pages = l.contains("pages/")
            && !after.starts_with("api/")
            && !after.starts_with('_')
            && matches!(after.rsplit('.').next(), Some("tsx" | "jsx" | "js" | "ts"));
        if !(is_app_page || is_pages) {
            continue;
        }
        let route_part = if is_app_page {
            after.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
        } else {
            after.rsplit_once('.').map(|(p, _)| p).unwrap_or(after)
        };
        let route: Vec<&str> = route_part
            .split('/')
            .filter(|s| !s.is_empty() && !(s.starts_with('(') && s.ends_with(')')) && *s != "index")
            .collect();
        push(
            out,
            EntryKind::Routes,
            format!("/{}", route.join("/")),
            &f.rel,
            1,
            "page",
        );
    }
}

/// First sentence of a description, at most 80 chars.
fn short(desc: &str) -> String {
    let d = desc.trim();
    let end = d
        .char_indices()
        .find(|(i, c)| matches!(c, '.' | ';') && d[i + 1..].starts_with(' '))
        .map(|(i, _)| i)
        .unwrap_or(d.len());
    clip(&d[..end], 80)
}

/// Render grouped by kind, then by file (paths are printed once):
///
/// ```text
/// ## mcp (2)
/// crates/x-mcp/src/server.rs
///   list_projects :91 — List all registered projects
/// ```
///
/// `max_lines` caps the entries listed (counts stay exact).
pub fn render(entries: &[EntryPoint], max_lines: usize) -> String {
    let mut out = String::new();
    if entries.is_empty() {
        out.push_str("No entry points found (rest, grpc, mcp, cli, routes, jobs).\n");
        return out;
    }
    let mut by_kind: BTreeMap<EntryKind, Vec<&EntryPoint>> = BTreeMap::new();
    for e in entries {
        by_kind.entry(e.kind).or_default().push(e);
    }
    let _ = writeln!(
        out,
        "{} entry points: {}",
        entries.len(),
        by_kind
            .iter()
            .map(|(k, v)| format!("{} {}", k.as_str(), v.len()))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut shown = 0usize;
    for (kind, items) in &by_kind {
        let _ = writeln!(out, "\n## {} ({})", kind.as_str(), items.len());
        let mut last_file = "";
        for e in items {
            if shown >= max_lines {
                break;
            }
            if e.file != last_file {
                let _ = writeln!(out, "{}", e.file);
                last_file = &e.file;
            }
            // gRPC details only repeat the proto path.
            let desc = if *kind == EntryKind::Grpc {
                String::new()
            } else {
                short(&e.description)
            };
            if desc.is_empty() {
                let _ = writeln!(out, "  {} :{}", e.name, e.line);
            } else {
                let _ = writeln!(out, "  {} :{} — {}", e.name, e.line, desc);
            }
            shown += 1;
        }
    }
    if entries.len() > shown {
        let _ = writeln!(
            out,
            "\n… +{} more (filter with kind=…, or raise max)",
            entries.len() - shown
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(rel: &str, text: &str) -> Vec<EntryPoint> {
        let mut out = Vec::new();
        extract_text(rel, text, &|_| true, &mut out);
        out
    }

    #[test]
    fn rust_mcp_tools_with_descriptions() {
        let src = r#"
    #[tool(
        description = "List all \"projects\""
    )]
    async fn list_projects(&self) -> R {}

    #[tool(description = "Start one")]
    pub async fn start_service(&self) -> R {}
"#;
        let e = run("src/server.rs", src);
        let names: Vec<(&str, &str)> = e
            .iter()
            .map(|e| (e.name.as_str(), e.description.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("list_projects", "List all \"projects\""),
                ("start_service", "Start one")
            ]
        );
        assert_eq!(e[0].line, 5);
    }

    #[test]
    fn clap_subcommands_with_names_and_docs() {
        let src = r#"
#[derive(Subcommand)]
enum Commands {
    /// Register a project
    Add {
        name: String,
    },
    #[command(name = "add-service")]
    AddService {
        x: u8,
    },
    /// Show status
    Status,
    Doctor(DoctorArgs),
}
"#;
        let e = run("src/main.rs", src);
        let got: Vec<(&str, &str)> = e
            .iter()
            .map(|e| (e.name.as_str(), e.description.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("add", "Register a project"),
                ("add-service", ""),
                ("status", "Show status"),
                ("doctor", "")
            ]
        );
    }

    #[test]
    fn ts_and_python_mcp_tools() {
        let ts = "server.tool(\"search_docs\", \"Search the docs\", {}, async () => {})\nserver.registerTool('get_page', { description: 'Fetch a page' }, h)";
        let e = run("src/index.ts", ts);
        assert_eq!(
            e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["search_docs", "get_page"]
        );
        assert_eq!(e[0].description, "Search the docs");
        let py = "@mcp.tool()\nasync def render_audio(x: int):\n    \"\"\"Render a song\"\"\"\n";
        let e = run("server.py", py);
        assert_eq!(
            (e[0].name.as_str(), e[0].description.as_str()),
            ("render_audio", "Render a song")
        );
    }

    #[test]
    fn cli_python_go_js() {
        let py = "sub = p.add_subparsers()\nsub.add_parser('index', help='Index a project')\n";
        assert_eq!(run("cli.py", py)[0].name, "index");
        let go =
            "var rootCmd = &cobra.Command{\n  Use: \"serve\",\n  Short: \"Run the server\",\n}";
        let e = run("cmd/root.go", go);
        assert_eq!(
            (e[0].name.as_str(), e[0].description.as_str()),
            ("serve", "Run the server")
        );
        let js = "const { program } = require('commander')\nprogram.command('build').description('Build it')";
        assert_eq!(run("bin/cli.js", js)[0].name, "build");
    }

    #[test]
    fn ui_routes_flutter_and_react() {
        let dart = "final r = GoRouter(routes: [\n  GoRoute(\n    path: '/profile/:id',\n    name: 'profile',\n    builder: (c, s) => P()),\n]);";
        let e = run("lib/router.dart", dart);
        assert_eq!(
            (e[0].name.as_str(), e[0].description.as_str()),
            ("/profile/:id", "profile")
        );
        let tsx = "<Routes><Route path=\"/settings\" element={<S/>} /></Routes>";
        assert_eq!(run("src/App.tsx", tsx)[0].name, "/settings");
        let files: Vec<SourceFile> = [
            "app/(shop)/cart/page.tsx",
            "app/page.tsx",
            "pages/about.tsx",
            "pages/_app.tsx",
            "pages/api/x.ts",
        ]
        .iter()
        .map(|r| SourceFile {
            rel: r.to_string(),
            size: 1,
        })
        .collect();
        let mut out = Vec::new();
        next_pages(&files, &mut out);
        let routes: Vec<&str> = out.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(routes, vec!["/cart", "/", "/about"]);
    }

    #[test]
    fn jobs_skip_comments() {
        let src = "// setInterval( in a comment\nconst t = setInterval(() => poll(), 5000)\nTimer.periodic(d, (_) {})";
        let e = run("a.ts", src);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].line, 2);
    }

    #[test]
    fn render_counts_and_caps() {
        let e: Vec<EntryPoint> = (0..5)
            .map(|i| EntryPoint {
                kind: EntryKind::Cli,
                name: format!("c{i}"),
                file: "m.rs".into(),
                line: i,
                description: String::new(),
            })
            .collect();
        let out = render(&e, 3);
        assert!(out.starts_with("5 entry points: cli 5"));
        assert!(
            out.contains("## cli (5)\nm.rs\n  c0 :0\n  c1 :1\n  c2 :2\n"),
            "{out}"
        );
        assert!(out.contains("… +2 more"));
        assert_eq!(
            short("Start all services. Returns immediately."),
            "Start all services"
        );
    }

    #[test]
    fn kinds_parse_aliases() {
        assert_eq!(EntryKind::parse("HTTP"), Some(EntryKind::Rest));
        assert_eq!(EntryKind::parse("screens"), Some(EntryKind::Routes));
        assert_eq!(EntryKind::parse("nope"), None);
    }
}
