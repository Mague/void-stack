//! `list_entrypoints`: every way into a project — HTTP routes, gRPC RPCs,
//! MCP tools, CLI subcommands, UI routes and background jobs — one line
//! each, extracted from the source with regexes (REST and gRPC reuse the
//! contract extractors).

use std::collections::{BTreeMap, HashMap};
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
    /// Desktop IPC: Tauri `#[tauri::command]`s registered in
    /// `generate_handler!` (what the web UI can `invoke`).
    Ipc,
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
            EntryKind::Ipc => "ipc",
            EntryKind::Routes => "routes",
            EntryKind::Jobs => "jobs",
        }
    }

    pub fn parse(s: &str) -> Option<EntryKind> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "rest" | "http" | "api" => EntryKind::Rest,
            "grpc" | "rpc" => EntryKind::Grpc,
            "mcp" | "tools" => EntryKind::Mcp,
            "cli" | "commands" | "flags" => EntryKind::Cli,
            "ipc" | "tauri" | "invoke" => EntryKind::Ipc,
            "routes" | "ui" | "pages" | "screens" => EntryKind::Routes,
            "jobs" | "cron" | "workers" => EntryKind::Jobs,
            _ => return None,
        })
    }

    pub const ALL: [EntryKind; 7] = [
        EntryKind::Rest,
        EntryKind::Grpc,
        EntryKind::Mcp,
        EntryKind::Cli,
        EntryKind::Ipc,
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

    // REST/gRPC need the scan too: it is how a client's vendored .proto
    // copies are told apart from services the project implements.
    let grpc_servers: std::collections::HashSet<String>;
    {
        let mut facts = Facts::default();
        for f in files.iter().filter(|f| scannable(f)) {
            let Some(text) = crate::fs_util::read_source(&root.join(&f.rel)) else {
                continue;
            };
            extract_text(&f.rel, &text, &want, &mut facts, &mut out);
        }
        grpc_servers = std::mem::take(&mut facts.grpc_servers);
        facts.resolve(&want, &mut out);
        if want(EntryKind::Routes) {
            next_pages(&files, &mut out);
        }
    }
    #[cfg(feature = "vector")]
    if want(EntryKind::Rest) || want(EntryKind::Grpc) {
        contracts(project, &want, &grpc_servers, &mut out);
    }
    #[cfg(not(feature = "vector"))]
    let _ = &grpc_servers;
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
    grpc_servers: &std::collections::HashSet<String>,
    out: &mut Vec<EntryPoint>,
) {
    use crate::vector_index::{ContractKind, ContractRole};
    let all = crate::vector_index::project_contracts(project);
    // A Flutter app keeps copies of the backend's .proto files to generate
    // its client: services it calls through generated stubs and never
    // implements are not its entry points (they were 1,475 of iunci's
    // 1,531 "entry points").
    let svc = |key: &str| key.split('.').next().unwrap_or(key).to_string();
    let consumed: std::collections::HashSet<String> = all
        .iter()
        .filter(|c| c.role == ContractRole::Consumer && c.kind == ContractKind::Grpc)
        .map(|c| svc(&c.key))
        .collect();
    let implements = |service: &str| {
        let base = service.strip_suffix("Service").unwrap_or(service);
        grpc_servers.contains(service) || grpc_servers.contains(base)
    };
    let client_copies: std::collections::HashSet<String> = all
        .iter()
        .filter(|c| c.role == ContractRole::Producer && c.kind == ContractKind::Grpc)
        .filter(|c| consumed.contains(&svc(&c.key)) && !implements(&svc(&c.key)))
        .map(|c| c.file.clone())
        .collect();
    let mut seen: std::collections::HashSet<(bool, String)> = Default::default();
    for c in all {
        if c.role != ContractRole::Producer || client_copies.contains(&c.file) {
            continue;
        }
        // The same proto vendored twice lists every RPC twice.
        if !seen.insert((c.kind == ContractKind::Grpc, c.key.clone())) {
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
// `(?s)`: a description continued with `\` + newline spans lines (the
// `.` of `\\.` must match the newline, or the whole match fails).
re!(DESCRIPTION, r#"(?s)description\s*=\s*"((?:[^"\\]|\\.)*)""#);
re!(LINE_CONTINUATION, r"\\\n\s*");
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
// `final Map<String, ToolFn> tools = { 'search': _search, … }`
re!(
    DART_REGISTRY_MAP,
    r"Map<String,\s*(\w*(?:Tool|Command)\w*)\s*>[^;{]*?\{"
);
re!(DART_MAP_KEY, r#"^\s*['"]([\w.\-]+)['"]\s*:"#);
// CLI flags read by hand / with package:args
re!(
    DART_ARGS_CONTAINS,
    r#"\b(?:args|arguments|argv|argResults)\.contains\(\s*['"](--?[\w\-]+)['"]"#
);
re!(DART_ADD_COMMAND, r#"\.addCommand\(\s*['"]([\w\-]+)['"]"#);
re!(
    DART_ADD_FLAG,
    r#"\.add(?:Flag|Option)\(\s*['"]([\w\-]+)['"]"#
);
// Started at launch without awaiting: `unawaited(sync.start())`
re!(DART_UNAWAITED, r"\bunawaited\(\s*([\w.]+)");
// Dart string constants: `static const home = '/home';`
re!(
    DART_CLASS,
    r"^\s*(?:abstract\s+|final\s+|sealed\s+)*class\s+(\w+)"
);
re!(
    DART_CONST,
    r#"^\s*(?:static\s+)?const\s+(?:String\s+)?(\w+)\s*=\s*['"]([^'"]*)['"]"#
);
re!(
    FLUTTER_NAMED_IDENT,
    r#"^\s*([A-Z]\w*\.\w+)\s*:\s*\(\s*(?:context|_|ctx)"#
);
re!(DART_CASE_ROUTE, r"^\s*case\s+([A-Z]\w*\.\w+)\s*:");
// Tauri
re!(
    TAURI_COMMAND_ATTR,
    r"^\s*#\[(?:tauri::)?command(?:\(.*\))?\]"
);
re!(TAURI_HANDLER, r"generate_handler!\s*\[");
// gRPC server implementations: Dart `extends FooServiceBase`, Go
// `RegisterFooServer(` / `UnimplementedFooServer`, Python
// `add_FooServicer_to_server`, tonic `FooServer::new(`.
re!(
    GRPC_SERVER_IMPL,
    r"(?:extends\s+(\w+?)ServiceBase\b|Register(\w+?)Server\(|Unimplemented(\w+?)Server\b|add_(\w+?)Servicer_to_server|\b(\w+?)Server::new\()"
);
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
re!(
    PATH_IDENT,
    r"\bpath\s*:\s*([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+|[a-z_]\w*)\s*[,)]"
);
re!(
    NAME_IDENT,
    r"\bname\s*:\s*([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+)\s*[,)]"
);
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

/// What one file can't settle alone, resolved once every file was read:
/// Dart route paths written as constants (`GoRoute(path: AppRoutes.home)`)
/// and Tauri commands (defined in one file, registered in another).
#[derive(Default)]
struct Facts {
    /// `AppRoutes.home` and bare `home` → `/home`.
    dart_consts: HashMap<String, String>,
    /// Bare names defined more than once (ambiguous without the class).
    dart_ambiguous: std::collections::HashSet<String>,
    /// (kind, ident, file, line, description)
    dart_routes: Vec<(String, String, usize, String)>,
    /// fn name → (file, line, doc)
    tauri_defs: HashMap<String, (String, usize, String)>,
    /// (fn name, file, line) as listed in `generate_handler![…]`
    tauri_handlers: Vec<(String, String, usize)>,
    /// gRPC services some file implements (server side).
    grpc_servers: std::collections::HashSet<String>,
}

impl Facts {
    fn dart_const(&mut self, class: Option<&str>, name: &str, value: &str) {
        if let Some(c) = class {
            self.dart_consts
                .insert(format!("{c}.{name}"), value.to_string());
        }
        if let Some(prev) = self.dart_consts.get(name)
            && prev != value
        {
            self.dart_ambiguous.insert(name.to_string());
        }
        self.dart_consts.insert(name.to_string(), value.to_string());
    }

    fn lookup(&self, ident: &str) -> Option<&String> {
        if let Some(v) = self.dart_consts.get(ident) {
            return Some(v);
        }
        // `Routes.home.path`, `HomeScreen.routeName`: fall back to the last
        // segment when only one constant has that name.
        let last = ident.rsplit('.').next()?;
        if self.dart_ambiguous.contains(last) {
            return None;
        }
        self.dart_consts.get(last)
    }

    fn resolve(self, want: &impl Fn(EntryKind) -> bool, out: &mut Vec<EntryPoint>) {
        if want(EntryKind::Routes) {
            for (ident, file, line, desc) in &self.dart_routes {
                // Unresolved constants are still routes: show the
                // identifier rather than dropping the screen.
                let name = self.lookup(ident).cloned().unwrap_or_else(|| ident.clone());
                let desc = self.lookup(desc).cloned().unwrap_or_else(|| desc.clone());
                push(out, EntryKind::Routes, name, file, *line, desc);
            }
        }
        if want(EntryKind::Ipc) {
            for (name, file, line) in &self.tauri_handlers {
                match self.tauri_defs.get(name) {
                    Some((f, l, doc)) => push(out, EntryKind::Ipc, name, f, *l, doc.clone()),
                    None => push(out, EntryKind::Ipc, name, file, *line, ""),
                }
            }
        }
    }
}

/// The part of a Rust file before its `#[cfg(test)]` module: fixtures in
/// tests (`#[tool(` inside a string) are not entry points.
fn without_rust_tests(text: &str) -> &str {
    let mut offset = 0;
    let mut cfg_test_at: Option<usize> = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("#[cfg(test)]") {
            cfg_test_at = Some(offset);
        } else if let Some(at) = cfg_test_at {
            // Only a test *module* ends the scan; a `#[cfg(test)] impl`
            // or helper fn sits between real code.
            if t.starts_with("mod ") {
                return &text[..at];
            }
            if !t.starts_with("#[") {
                cfg_test_at = None;
            }
        }
        offset += line.len();
    }
    text
}

/// Items of the `{ … }` block opened at byte `open` (the `{`), up to its
/// matching `}`.
fn brace_block(text: &str, open: usize) -> &str {
    let mut depth = 0i32;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open + 1..open + i];
                }
            }
            _ => {}
        }
    }
    &text[open + 1..]
}

/// Text of one `GoRoute(` call: up to 300 chars, stopping at the next
/// `GoRoute(` (a nested or sibling route must not lend it its path).
fn go_route_tail(text: &str, start: usize) -> &str {
    let after = start + "GoRoute(".len();
    let end = text[after.min(text.len())..]
        .find("GoRoute(")
        .map(|i| after + i)
        .unwrap_or(text.len())
        .min(start + 300)
        .min(text.len());
    &text[start..end]
}

fn dart_is_entry_file(rel: &str) -> bool {
    let l = rel.to_ascii_lowercase();
    l.ends_with("/main.dart") || l == "main.dart" || l.starts_with("bin/") || l.contains("/bin/")
}

/// Dart: constants, routes written with them, tool/command registries,
/// hand-parsed flags and jobs started at launch.
fn extract_dart(
    rel: &str,
    text: &str,
    lines: &[&str],
    want: &impl Fn(EntryKind) -> bool,
    facts: &mut Facts,
    out: &mut Vec<EntryPoint>,
) {
    let mut class: Option<&str> = None;
    for l in lines {
        if let Some(c) = DART_CLASS.captures(l) {
            class = c.get(1).map(|m| m.as_str());
        }
        if let Some(c) = DART_CONST.captures(l) {
            facts.dart_const(class, &c[1], &c[2]);
        }
    }

    if want(EntryKind::Routes) {
        for m in GO_ROUTE.find_iter(text) {
            let tail = go_route_tail(text, m.start());
            if PATH_PROP.is_match(tail) {
                continue; // literal path: handled with the other routes
            }
            if let Some(p) = PATH_IDENT.captures(tail) {
                let name = NAME_IDENT
                    .captures(tail)
                    .map(|n| n[1].to_string())
                    .or_else(|| NAME_PROP.captures(tail).map(|n| n[1].to_string()))
                    .unwrap_or_default();
                facts.dart_routes.push((
                    p[1].to_string(),
                    rel.to_string(),
                    line_of(text, m.start()),
                    name,
                ));
            }
        }
        let switch_routes = text.contains("onGenerateRoute") || text.contains("RouteSettings");
        for (i, l) in lines.iter().enumerate() {
            let ident = FLUTTER_NAMED_IDENT
                .captures(l)
                .or_else(|| switch_routes.then(|| DART_CASE_ROUTE.captures(l)).flatten());
            if let Some(c) = ident {
                facts
                    .dart_routes
                    .push((c[1].to_string(), rel.to_string(), i + 1, String::new()));
            }
        }
    }

    if want(EntryKind::Mcp) || want(EntryKind::Cli) {
        for c in DART_REGISTRY_MAP.captures_iter(text) {
            let ty = &c[1];
            let kind = if ty.contains("Tool") {
                EntryKind::Mcp
            } else {
                EntryKind::Cli
            };
            if !want(kind) {
                continue;
            }
            let Some(m) = c.get(0) else { continue };
            let open = m.end() - 1;
            let block = brace_block(text, open);
            let base = line_of(text, open);
            for (k, l) in block.lines().enumerate() {
                if let Some(key) = DART_MAP_KEY.captures(l) {
                    push(
                        out,
                        kind,
                        &key[1],
                        rel,
                        base + k,
                        format!("registered in {ty} map"),
                    );
                }
            }
        }
    }

    if want(EntryKind::Cli) {
        for re in [&*DART_ARGS_CONTAINS, &*DART_ADD_COMMAND] {
            for c in re.captures_iter(text) {
                let m = c.get(0).map(|m| m.start()).unwrap_or(0);
                push(out, EntryKind::Cli, &c[1], rel, line_of(text, m), "");
            }
        }
        for c in DART_ADD_FLAG.captures_iter(text) {
            let m = c.get(0).map(|m| m.start()).unwrap_or(0);
            push(
                out,
                EntryKind::Cli,
                format!("--{}", &c[1]),
                rel,
                line_of(text, m),
                "",
            );
        }
    }

    if want(EntryKind::Jobs) && dart_is_entry_file(rel) {
        for (i, l) in lines.iter().enumerate() {
            if l.trim_start().starts_with("//") {
                continue;
            }
            if let Some(c) = DART_UNAWAITED.captures(l) {
                push(out, EntryKind::Jobs, &c[1], rel, i + 1, l.trim());
            }
        }
    }
}

/// Tauri: `#[tauri::command]` fns and the names `generate_handler![…]`
/// registers.
fn extract_tauri(rel: &str, text: &str, lines: &[&str], facts: &mut Facts) {
    if !text.contains("command") {
        return;
    }
    for (i, l) in lines.iter().enumerate() {
        if !TAURI_COMMAND_ATTR.is_match(l) {
            continue;
        }
        if let Some((j, name)) = lines
            .iter()
            .enumerate()
            .skip(i + 1)
            .take(4)
            .find_map(|(j, l)| RS_FN_NAME.captures(l).map(|c| (j, c[1].to_string())))
        {
            facts
                .tauri_defs
                .insert(name, (rel.to_string(), j + 1, doc_above(lines, i)));
        }
    }
    for m in TAURI_HANDLER.find_iter(text) {
        let open = m.end() - 1;
        let block = brace_block(text, open);
        let base = line_of(text, open);
        for (k, l) in block.lines().enumerate() {
            for item in l.split(',') {
                let item = item.trim();
                if item.is_empty() || item.starts_with("//") {
                    continue;
                }
                let name = item.rsplit("::").next().unwrap_or(item);
                if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    facts
                        .tauri_handlers
                        .push((name.to_string(), rel.to_string(), base + k));
                }
            }
        }
    }
}

fn extract_text(
    rel: &str,
    text: &str,
    want: &impl Fn(EntryKind) -> bool,
    facts: &mut Facts,
    out: &mut Vec<EntryPoint>,
) {
    let lang = language(rel).unwrap_or("");
    let text = if lang == "Rust" {
        without_rust_tests(text)
    } else {
        text
    };
    let lines: Vec<&str> = text.lines().collect();
    if text.contains("Server") || text.contains("ServiceBase") || text.contains("Servicer") {
        for c in GRPC_SERVER_IMPL.captures_iter(text) {
            if let Some(m) = c.iter().skip(1).flatten().next() {
                facts.grpc_servers.insert(m.as_str().to_string());
                facts.grpc_servers.insert(format!("{}Service", m.as_str()));
            }
        }
    }
    if lang == "Dart" {
        extract_dart(rel, text, &lines, want, facts, out);
    }
    if lang == "Rust" && want(EntryKind::Ipc) {
        extract_tauri(rel, text, &lines, facts);
    }

    if want(EntryKind::Mcp) {
        match lang {
            "Rust" if text.contains("#[tool(") => {
                for (i, l) in lines.iter().enumerate() {
                    if !RS_TOOL_ATTR.is_match(l) {
                        continue;
                    }
                    // The attribute may span lines; the fn follows within ~20.
                    let window = lines[i..(i + 20).min(lines.len())].join("\n");
                    let desc = DESCRIPTION
                        .captures(&window)
                        .map(|c| {
                            LINE_CONTINUATION
                                .replace_all(&c[1], "")
                                .replace("\\\"", "\"")
                        })
                        .unwrap_or_default();
                    if let Some((j, name)) = lines
                        .iter()
                        .enumerate()
                        .skip(i + 1)
                        .take(20)
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
                        let tail = go_route_tail(text, m.start());
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
        out.push_str("No entry points found (rest, grpc, mcp, cli, ipc, routes, jobs).\n");
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
        let mut facts = Facts::default();
        extract_text(rel, text, &|_| true, &mut facts, &mut out);
        facts.resolve(&|_| true, &mut out);
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
    fn rust_mcp_description_continued_over_lines() {
        let src = "    #[tool(\n        description = \"Run a full \\\n                       analysis now\"\n    )]\n    async fn full_analysis(&self) -> R {}\n";
        let e = run("src/server.rs", src);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].description, "Run a full analysis now");
    }

    #[test]
    fn rust_test_module_is_not_scanned() {
        let src = "#[cfg(test)]\nimpl X { fn helper() {} }\n\n#[tool(description = \"real\")]\nasync fn real_tool() {}\n\n#[cfg(test)]\nmod tests {\n    const F: &str = r#\"\n#[tool(description = \"fake\")]\nasync fn fixture_tool() {}\n\"#;\n}\n";
        let names: Vec<String> = run("src/server.rs", src)
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["real_tool"]);
    }

    #[test]
    fn dart_go_router_paths_from_constants() {
        let routes = "class AppRoutes {\n  static const String home = '/home';\n  static const profile = '/profile/:id';\n}\n";
        let router = "final router = GoRouter(routes: [\n  GoRoute(\n    path: AppRoutes.home,\n    builder: (c, s) => const HomePage(),\n  ),\n  GoRoute(path: AppRoutes.profile, name: AppRoutes.home),\n  GoRoute(path: Unknown.screen),\n  GoRoute(path: '/literal'),\n]);\n";
        let mut out = Vec::new();
        let mut facts = Facts::default();
        let all = |_| true;
        extract_text("lib/router/routes.dart", routes, &all, &mut facts, &mut out);
        extract_text("lib/router/router.dart", router, &all, &mut facts, &mut out);
        facts.resolve(&all, &mut out);
        let mut names: Vec<&str> = out
            .iter()
            .filter(|e| e.kind == EntryKind::Routes)
            .map(|e| e.name.as_str())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["/home", "/literal", "/profile/:id", "Unknown.screen"]
        );
    }

    #[test]
    fn dart_tool_registry_flags_and_startup_jobs() {
        let src = "typedef ToolFn = Future<String> Function(Map args);\nfinal Map<String, ToolFn> tools = {\n  'search_web': _searchWeb,\n  'fetch_page': (a) => fetch(a),\n};\n\nvoid main(List<String> args) {\n  if (args.contains('--mcp')) { runMcp(); }\n  final p = ArgParser()..addCommand('serve')..addFlag('verbose');\n  unawaited(syncService.start());\n  // unawaited(commented.out());\n}\n";
        let e = run("bin/main.dart", src);
        let got: Vec<(EntryKind, &str)> = e.iter().map(|e| (e.kind, e.name.as_str())).collect();
        for want in [
            (EntryKind::Mcp, "search_web"),
            (EntryKind::Mcp, "fetch_page"),
            (EntryKind::Cli, "--mcp"),
            (EntryKind::Cli, "serve"),
            (EntryKind::Cli, "--verbose"),
            (EntryKind::Jobs, "syncService.start"),
        ] {
            assert!(got.contains(&want), "missing {want:?} in {got:?}");
        }
        assert!(!got.iter().any(|(_, n)| *n == "commented.out"));
        assert_eq!(e.iter().find(|e| e.name == "fetch_page").unwrap().line, 4);
    }

    #[test]
    fn tauri_commands_registered_in_generate_handler() {
        let cmds = "/// Start a project\n#[tauri::command]\npub async fn start_project(name: String) -> R {}\n\n#[tauri::command(rename_all = \"snake_case\")]\nfn stop_all() {}\n";
        let main = "fn main() {\n    tauri::Builder::default()\n        .invoke_handler(tauri::generate_handler![\n            commands::start_project,\n            commands::stop_all,\n            missing_def,\n        ])\n        .run();\n}\n";
        let mut out = Vec::new();
        let mut facts = Facts::default();
        let all = |_| true;
        extract_text("src/commands.rs", cmds, &all, &mut facts, &mut out);
        extract_text("src/main.rs", main, &all, &mut facts, &mut out);
        facts.resolve(&all, &mut out);
        let ipc: Vec<(&str, &str, usize, &str)> = out
            .iter()
            .filter(|e| e.kind == EntryKind::Ipc)
            .map(|e| {
                (
                    e.name.as_str(),
                    e.file.as_str(),
                    e.line,
                    e.description.as_str(),
                )
            })
            .collect();
        assert_eq!(
            ipc,
            vec![
                ("start_project", "src/commands.rs", 3, "Start a project"),
                ("stop_all", "src/commands.rs", 6, ""),
                ("missing_def", "src/main.rs", 6, ""),
            ]
        );
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
