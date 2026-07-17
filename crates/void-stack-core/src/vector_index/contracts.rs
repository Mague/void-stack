//! API-contract extraction and cross-project matching.
//!
//! Cross-project links derived from real API contracts instead of shared
//! symbol names:
//! - **gRPC**: `.proto` service/rpc definitions (producers) matched against
//!   generated stubs — Dart `*.pbgrpc.dart` client literals and Go
//!   `*_grpc.pb.go` `Invoke(...)` (consumer) / `FullMethod:` (producer)
//!   literals. Vendored copies of the same `.proto` (by content hash) are
//!   themselves a link.
//! - **REST**: route producers (existing api_routes scanners + Next.js app
//!   router + Go net/http, gin, echo, chi) matched against client-side
//!   HTTP calls (`fetch`, axios, Dio) with path params normalized to
//!   `{param}`.
//!
//! Extraction is cached per project in a sidecar JSON keyed by file SHA-256
//! (`<index_dir>/contracts.json`) so cross searches don't re-scan unchanged
//! files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::fs_util::file_sha256;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

// ── Model ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractKind {
    /// A gRPC `Service.Rpc` pair.
    Grpc,
    /// A whole `.proto` file identified by content hash (vendored copies).
    GrpcProtoHash,
    /// An HTTP route (`METHOD /normalized/path`).
    Rest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractRole {
    /// Defines/implements the endpoint (proto def, route handler, gRPC server).
    Producer,
    /// Calls the endpoint (generated client stub, fetch/axios/Dio call).
    Consumer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiContract {
    pub kind: ContractKind,
    pub role: ContractRole,
    /// Match key: gRPC `Service.Rpc`, proto-hash `sha256:<12 hex>`,
    /// REST `METHOD /normalized/path`.
    pub key: String,
    /// Human-readable origin (proto file, framework, raw path).
    pub detail: String,
    pub file: String,
    pub line: usize,
}

/// A contract-level match between two projects.
#[derive(Debug, Clone, Serialize)]
pub struct ContractLink {
    /// e.g. `grpc: AuthService.Login`, `rest: POST /api/v1/orders`,
    /// `grpc proto: auth.proto (shared definition)`.
    pub via: String,
    /// Matched contract keys (deduped).
    pub keys: Vec<String>,
    /// `high` for exact matches, `medium` for param-normalized matches.
    pub confidence: &'static str,
    /// Consumer side file:line (call site) when known.
    pub consumer_site: Option<String>,
    /// Producer side file:line (handler) when known.
    pub producer_site: Option<String>,
}

// ── Public API ──────────────────────────────────────────────

/// Extract (with caching) every API contract a project produces/consumes.
pub fn project_contracts(project: &Project) -> Vec<ApiContract> {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    let mut cache = ContractCache::load(project);
    let files = collect_candidate_files(&root);

    let mut out: Vec<ApiContract> = Vec::new();
    let mut fresh: HashMap<String, CachedFile> = HashMap::new();

    for rel in &files {
        let abs = root.join(rel);
        let hash = file_sha256(&abs);
        let entry = match cache.files.remove(rel) {
            Some(cached) if cached.hash == hash && !hash.is_empty() => cached,
            _ => CachedFile {
                hash,
                contracts: extract_file_contracts(rel, &abs),
            },
        };
        out.extend(entry.contracts.iter().cloned());
        fresh.insert(rel.clone(), entry);
    }

    // Files that disappeared are simply not re-inserted.
    let updated = ContractCache { files: fresh };
    updated.save(project);
    out
}

/// Match contracts of two projects: a consumer in one side against a
/// producer in the other (both directions). Pure — fully unit-testable.
pub fn contract_links(a: &[ApiContract], b: &[ApiContract]) -> Vec<ContractLink> {
    let mut links: Vec<ContractLink> = Vec::new();

    // gRPC + proto-hash: exact key, opposite roles (proto-hash links on
    // key equality regardless of role — a vendored copy is a link per se).
    link_exact(a, b, ContractKind::Grpc, &mut links);
    link_exact(b, a, ContractKind::Grpc, &mut links);
    link_proto_hash(a, b, &mut links);

    // REST: exact first, then param-normalized segment match.
    link_rest(a, b, &mut links);
    link_rest(b, a, &mut links);

    // Dedupe by via.
    let mut seen = std::collections::HashSet::new();
    links.retain(|l| seen.insert(l.via.clone()));
    links
}

// ── Matching internals ──────────────────────────────────────

fn link_exact(
    consumers: &[ApiContract],
    producers: &[ApiContract],
    kind: ContractKind,
    links: &mut Vec<ContractLink>,
) {
    for c in consumers
        .iter()
        .filter(|c| c.kind == kind && c.role == ContractRole::Consumer)
    {
        for p in producers
            .iter()
            .filter(|p| p.kind == kind && p.role == ContractRole::Producer)
        {
            if c.key == p.key {
                links.push(ContractLink {
                    via: format!("grpc: {}", c.key),
                    keys: vec![c.key.clone()],
                    confidence: "high",
                    consumer_site: Some(format!("{}:{}", c.file, c.line)),
                    producer_site: Some(format!("{}:{}", p.file, p.line)),
                });
            }
        }
    }
}

fn link_proto_hash(a: &[ApiContract], b: &[ApiContract], links: &mut Vec<ContractLink>) {
    for ca in a.iter().filter(|c| c.kind == ContractKind::GrpcProtoHash) {
        for cb in b.iter().filter(|c| c.kind == ContractKind::GrpcProtoHash) {
            if ca.key == cb.key {
                links.push(ContractLink {
                    via: format!("grpc proto: {} (shared definition)", ca.detail),
                    keys: vec![ca.key.clone()],
                    confidence: "high",
                    consumer_site: Some(format!("{}:{}", cb.file, cb.line)),
                    producer_site: Some(format!("{}:{}", ca.file, ca.line)),
                });
            }
        }
    }
}

fn link_rest(consumers: &[ApiContract], producers: &[ApiContract], links: &mut Vec<ContractLink>) {
    for c in consumers
        .iter()
        .filter(|c| c.kind == ContractKind::Rest && c.role == ContractRole::Consumer)
    {
        for p in producers
            .iter()
            .filter(|p| p.kind == ContractKind::Rest && p.role == ContractRole::Producer)
        {
            if c.key == p.key {
                links.push(ContractLink {
                    via: format!("rest: {}", c.key),
                    keys: vec![c.key.clone()],
                    confidence: "high",
                    consumer_site: Some(format!("{}:{}", c.file, c.line)),
                    producer_site: Some(format!("{}:{}", p.file, p.line)),
                });
            } else if rest_keys_match_normalized(&c.key, &p.key) {
                links.push(ContractLink {
                    via: format!("rest: {} ~ {}", c.key, p.key),
                    keys: vec![c.key.clone(), p.key.clone()],
                    confidence: "medium",
                    consumer_site: Some(format!("{}:{}", c.file, c.line)),
                    producer_site: Some(format!("{}:{}", p.file, p.line)),
                });
            }
        }
    }
}

/// Same method + segment-wise path match where `{param}` on either side
/// matches any single segment. `POST /users/{param}` ~ `POST /users/123`.
fn rest_keys_match_normalized(a: &str, b: &str) -> bool {
    let (ma, pa) = match a.split_once(' ') {
        Some(x) => x,
        None => return false,
    };
    let (mb, pb) = match b.split_once(' ') {
        Some(x) => x,
        None => return false,
    };
    if ma != mb {
        return false;
    }
    let sa: Vec<&str> = pa.trim_matches('/').split('/').collect();
    let sb: Vec<&str> = pb.trim_matches('/').split('/').collect();
    if sa.len() != sb.len() || sa.is_empty() {
        return false;
    }
    // At least one concrete (non-param) segment must agree, otherwise
    // `/{param}` would match everything.
    let mut concrete_agree = false;
    for (x, y) in sa.iter().zip(sb.iter()) {
        let xp = *x == "{param}";
        let yp = *y == "{param}";
        if !xp && !yp {
            if x != y {
                return false;
            }
            concrete_agree = true;
        }
    }
    concrete_agree
}

// ── File walking ────────────────────────────────────────────

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".next",
    ".nuxt",
    "vendor",
    ".dart_tool",
    ".gradle",
    ".idea",
    "coverage",
    ".void-stack",
];

const MAX_DEPTH: u32 = 7;
const MAX_FILE_BYTES: u64 = 1_048_576;

fn collect_candidate_files(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<String>) {
        if depth > MAX_DEPTH {
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if name.starts_with('.') && name != ".well-known"
                    || SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s))
                {
                    continue;
                }
                walk(root, &path, depth + 1, out);
                continue;
            }
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let is_candidate = matches!(
                ext.as_str(),
                "proto" | "dart" | "go" | "js" | "ts" | "jsx" | "tsx" | "py"
            );
            if !is_candidate {
                continue;
            }
            if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out.sort();
    out
}

// ── Per-file extraction ─────────────────────────────────────

fn extract_file_contracts(rel: &str, abs: &Path) -> Vec<ApiContract> {
    let Ok(content) = std::fs::read_to_string(abs) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let lower = rel.to_lowercase();

    if lower.ends_with(".proto") {
        extract_proto_producers(rel, &content, &mut out);
    } else if lower.ends_with(".pbgrpc.dart") {
        extract_grpc_literals(rel, &content, ContractRole::Consumer, &mut out);
    } else if lower.ends_with("_grpc.pb.go") || lower.ends_with(".pb.go") {
        extract_go_grpc_generated(rel, &content, &mut out);
    } else if lower.ends_with(".go") {
        extract_go_routes(rel, &content, &mut out);
        extract_rest_consumers(rel, &content, &mut out);
    } else if lower.ends_with(".dart") {
        extract_rest_consumers(rel, &content, &mut out);
    } else if is_next_route_file(&lower) {
        extract_next_route(rel, &content, &mut out);
        extract_rest_consumers(rel, &content, &mut out);
    } else if matches!(
        lower.rsplit('.').next().unwrap_or(""),
        "js" | "ts" | "jsx" | "tsx"
    ) {
        extract_express_routes(rel, &content, &mut out);
        extract_rest_consumers(rel, &content, &mut out);
    } else if lower.ends_with(".py") {
        extract_python_routes(rel, &content, &mut out);
    }

    out
}

// ── gRPC extraction ─────────────────────────────────────────

fn extract_proto_producers(rel: &str, content: &str, out: &mut Vec<ApiContract>) {
    static RPC_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*rpc\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap());
    static SVC_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*service\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());

    let mut current_service = String::new();
    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = SVC_RE.captures(line) {
            current_service = cap[1].to_string();
            continue;
        }
        if !current_service.is_empty()
            && let Some(cap) = RPC_RE.captures(line)
        {
            out.push(ApiContract {
                kind: ContractKind::Grpc,
                role: ContractRole::Producer,
                key: format!("{}.{}", current_service, &cap[1]),
                detail: format!("proto: {}", rel),
                file: rel.to_string(),
                line: i + 1,
            });
        }
        if line.trim() == "}" && !line.contains("rpc") {
            // A closing brace at top level usually ends the service block;
            // protos rarely nest, so this cheap reset is good enough.
            if line.trim_start() == line.trim_end() && !line.starts_with(' ') {
                current_service.clear();
            }
        }
    }

    // Whole-file hash — vendored copies of the same proto are a link.
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    out.push(ApiContract {
        kind: ContractKind::GrpcProtoHash,
        role: ContractRole::Producer,
        key: format!("sha256:{}", &digest[..12]),
        detail: rel.rsplit('/').next().unwrap_or(rel).to_string(),
        file: rel.to_string(),
        line: 1,
    });
}

/// `/package.Service/Method` literals — present in every generated stub
/// (Dart `*.pbgrpc.dart`, Go `*_grpc.pb.go`).
fn grpc_literal_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"['"]/(?:[A-Za-z0-9_.]+\.)?([A-Za-z_][A-Za-z0-9_]*)/([A-Za-z_][A-Za-z0-9_]*)['"]"#,
        )
        .unwrap()
    });
    &RE
}

fn extract_grpc_literals(rel: &str, content: &str, role: ContractRole, out: &mut Vec<ApiContract>) {
    for (i, line) in content.lines().enumerate() {
        for cap in grpc_literal_re().captures_iter(line) {
            out.push(ApiContract {
                kind: ContractKind::Grpc,
                role,
                key: format!("{}.{}", &cap[1], &cap[2]),
                detail: format!("generated stub: {}", rel),
                file: rel.to_string(),
                line: i + 1,
            });
        }
    }
}

/// Go generated gRPC code carries both sides: `.Invoke(ctx, "/x.Y/Z"` in
/// the client section, `FullMethod: "/x.Y/Z"` in the server section.
fn extract_go_grpc_generated(rel: &str, content: &str, out: &mut Vec<ApiContract>) {
    for (i, line) in content.lines().enumerate() {
        let role = if line.contains("FullMethod") {
            ContractRole::Producer
        } else if line.contains(".Invoke(") || line.contains("NewStream(") {
            ContractRole::Consumer
        } else {
            continue;
        };
        for cap in grpc_literal_re().captures_iter(line) {
            out.push(ApiContract {
                kind: ContractKind::Grpc,
                role,
                key: format!("{}.{}", &cap[1], &cap[2]),
                detail: format!("generated go: {}", rel),
                file: rel.to_string(),
                line: i + 1,
            });
        }
    }
}

// ── REST producers ──────────────────────────────────────────

/// Go route registration: gin/echo (`r.GET("/path"`), chi (`r.Get(`),
/// net/http (`http.HandleFunc("/path"`).
fn extract_go_routes(rel: &str, content: &str, out: &mut Vec<ApiContract>) {
    static GIN_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\.(GET|POST|PUT|PATCH|DELETE|Get|Post|Put|Patch|Delete)\(\s*"([^"]+)""#)
            .unwrap()
    });
    static HANDLE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\bHandleFunc\(\s*"(?:(GET|POST|PUT|PATCH|DELETE)\s+)?([^"]+)""#).unwrap()
    });

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = GIN_RE.captures(line)
            && let Some(path) = normalize_rest_path(&cap[2])
        {
            out.push(rest_contract(
                ContractRole::Producer,
                &cap[1].to_uppercase(),
                &path,
                &cap[2],
                rel,
                i + 1,
            ));
        }
        if let Some(cap) = HANDLE_RE.captures(line) {
            let method = cap.get(1).map(|m| m.as_str()).unwrap_or("GET");
            if let Some(path) = normalize_rest_path(&cap[2]) {
                out.push(rest_contract(
                    ContractRole::Producer,
                    method,
                    &path,
                    &cap[2],
                    rel,
                    i + 1,
                ));
            }
        }
    }
}

/// Next.js app router: `app/api/orders/[id]/route.ts` exporting
/// `GET`/`POST`/... — the path comes from the directory structure.
fn is_next_route_file(rel_lower: &str) -> bool {
    (rel_lower.ends_with("/route.ts")
        || rel_lower.ends_with("/route.js")
        || rel_lower.ends_with("/route.tsx"))
        && rel_lower.contains("app/")
}

fn extract_next_route(rel: &str, content: &str, out: &mut Vec<ApiContract>) {
    static EXPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"export\s+(?:async\s+)?(?:function|const)\s+(GET|POST|PUT|PATCH|DELETE)\b")
            .unwrap()
    });

    // Derive the URL path: everything after the `app/` segment, minus the
    // trailing route.ts, with `[id]` → `{param}` and `(group)` removed.
    let norm = rel.replace('\\', "/");
    let Some(idx) = norm.find("app/") else { return };
    let after = &norm[idx + 3..]; // keep leading '/'
    let dir = after
        .trim_end_matches("/route.ts")
        .trim_end_matches("/route.js")
        .trim_end_matches("/route.tsx");
    let path: String = dir
        .split('/')
        .filter(|s| !(s.is_empty() || s.starts_with('(') && s.ends_with(')')))
        .map(|s| {
            if s.starts_with('[') && s.ends_with(']') {
                "{param}".to_string()
            } else {
                s.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    let url = format!("/{}", path);

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = EXPORT_RE.captures(line) {
            out.push(rest_contract(
                ContractRole::Producer,
                &cap[1],
                &url,
                rel,
                rel,
                i + 1,
            ));
        }
    }
}

/// Express-style: `app.get('/path', handler)` / `router.post(...)`.
fn extract_express_routes(rel: &str, content: &str, out: &mut Vec<ApiContract>) {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\b(?:app|router)\.(get|post|put|patch|delete)\(\s*['"`]([^'"`]+)['"`]"#)
            .unwrap()
    });
    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = RE.captures(line)
            && let Some(path) = normalize_rest_path(&cap[2])
        {
            out.push(rest_contract(
                ContractRole::Producer,
                &cap[1].to_uppercase(),
                &path,
                &cap[2],
                rel,
                i + 1,
            ));
        }
    }
}

/// FastAPI/Flask decorators: `@app.get("/path")`, `@router.post(...)`,
/// `@app.route("/path", methods=["POST"])`.
fn extract_python_routes(rel: &str, content: &str, out: &mut Vec<ApiContract>) {
    static DECO_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"@\w+\.(get|post|put|patch|delete)\(\s*['"]([^'"]+)['"]"#).unwrap()
    });
    static ROUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"@\w+\.route\(\s*['"]([^'"]+)['"](?:.*methods\s*=\s*\[['"](\w+)['"])?"#)
            .unwrap()
    });
    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = DECO_RE.captures(line)
            && let Some(path) = normalize_rest_path(&cap[2])
        {
            out.push(rest_contract(
                ContractRole::Producer,
                &cap[1].to_uppercase(),
                &path,
                &cap[2],
                rel,
                i + 1,
            ));
        } else if let Some(cap) = ROUTE_RE.captures(line) {
            let method = cap.get(2).map(|m| m.as_str().to_uppercase());
            if let Some(path) = normalize_rest_path(&cap[1]) {
                out.push(rest_contract(
                    ContractRole::Producer,
                    method.as_deref().unwrap_or("GET"),
                    &path,
                    &cap[1],
                    rel,
                    i + 1,
                ));
            }
        }
    }
}

// ── REST consumers ──────────────────────────────────────────

/// Client-side HTTP calls: `fetch('/api/...')` (+ same-statement
/// `method: 'POST'`), `axios.get('/x')`, Dio `_dio.post('/x')` and any
/// `<recv>.get('/x')` whose argument looks like a URL path.
fn extract_rest_consumers(rel: &str, content: &str, out: &mut Vec<ApiContract>) {
    static FETCH_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\bfetch\(\s*[`'"]([^`'"]+)[`'"]"#).unwrap());
    static METHOD_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"method:\s*['"](\w+)['"]"#).unwrap());
    static VERB_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\.(get|post|put|patch|delete)(?:<[^>]*>)?\(\s*[`'"]([^`'"]+)[`'"]"#).unwrap()
    });

    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        // fetch(...) — look ahead a few lines for an explicit method.
        if let Some(cap) = FETCH_RE.captures(line)
            && let Some(path) = normalize_rest_path(&cap[1])
        {
            let window = lines[i..lines.len().min(i + 4)].join(" ");
            let method = METHOD_RE
                .captures(&window)
                .map(|m| m[1].to_uppercase())
                .unwrap_or_else(|| "GET".to_string());
            out.push(rest_contract(
                ContractRole::Consumer,
                &method,
                &path,
                &cap[1],
                rel,
                i + 1,
            ));
        }

        // axios.get / dio.post / api.delete — receiver-agnostic, but the
        // argument must look like a URL path so `map.get('key')` is skipped.
        // Express/gin registrations are excluded: their line carries a
        // handler signature (`(req`, `function(`, `ctx *gin.Context`).
        if let Some(cap) = VERB_RE.captures(line) {
            let raw = &cap[2];
            let looks_like_handler = line.contains("(req")
                || line.contains("function(")
                || line.contains("=> {") && line.contains("res")
                || line.contains("Context");
            if !looks_like_handler
                && (raw.starts_with('/') || raw.starts_with("http"))
                && let Some(path) = normalize_rest_path(raw)
            {
                out.push(rest_contract(
                    ContractRole::Consumer,
                    &cap[1].to_uppercase(),
                    &path,
                    raw,
                    rel,
                    i + 1,
                ));
            }
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────

fn rest_contract(
    role: ContractRole,
    method: &str,
    normalized_path: &str,
    raw: &str,
    file: &str,
    line: usize,
) -> ApiContract {
    ApiContract {
        kind: ContractKind::Rest,
        role,
        key: format!("{} {}", method, normalized_path),
        detail: raw.to_string(),
        file: file.to_string(),
        line,
    }
}

/// Normalize a route/URL path for matching:
/// - strips scheme+host from absolute URLs and any query string
/// - `:id`, `{id}`, `[id]`, `<id>`, `${expr}` → `{param}`
/// - drops trailing slash
///
/// Returns `None` when the value doesn't look like a URL path (so map keys
/// passed to `.get('key')` never become contracts).
pub fn normalize_rest_path(raw: &str) -> Option<String> {
    let mut p = raw.trim().to_string();
    if let Some(rest) = p
        .strip_prefix("http://")
        .or_else(|| p.strip_prefix("https://"))
    {
        p = rest[rest.find('/')?..].to_string();
    }
    if let Some(q) = p.find('?') {
        p.truncate(q);
    }
    if !p.starts_with('/') || p.len() < 2 {
        return None;
    }
    let segments: Vec<String> = p
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            let is_param = seg.starts_with(':')
                || (seg.starts_with('{') && seg.ends_with('}'))
                || (seg.starts_with('[') && seg.ends_with(']'))
                || (seg.starts_with('<') && seg.ends_with('>'))
                || seg.contains("${");
            if is_param {
                "{param}".to_string()
            } else {
                seg.to_string()
            }
        })
        .collect();
    if segments.is_empty() {
        return None;
    }
    Some(format!("/{}", segments.join("/")))
}

// ── Cache ───────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct ContractCache {
    files: HashMap<String, CachedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedFile {
    hash: String,
    contracts: Vec<ApiContract>,
}

impl ContractCache {
    fn path(project: &Project) -> PathBuf {
        super::stats::index_dir(project).join("contracts.json")
    }

    fn load(project: &Project) -> Self {
        std::fs::read_to_string(Self::path(project))
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    fn save(&self, project: &Project) {
        let path = Self::path(project);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(kind: ContractKind, role: ContractRole, key: &str) -> ApiContract {
        ApiContract {
            kind,
            role,
            key: key.to_string(),
            detail: String::new(),
            file: "f".to_string(),
            line: 1,
        }
    }

    #[test]
    fn test_proto_producer_extraction() {
        crate::isolate_test_data_dir();
        let proto = r#"syntax = "proto3";
package auth;

service AuthService {
  rpc Login(LoginRequest) returns (LoginResponse);
  rpc Logout(LogoutRequest) returns (Empty);
}
"#;
        let mut out = Vec::new();
        extract_proto_producers("proto/auth.proto", proto, &mut out);
        let keys: Vec<&str> = out.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"AuthService.Login"), "got {:?}", keys);
        assert!(keys.contains(&"AuthService.Logout"), "got {:?}", keys);
        assert!(
            out.iter().any(|c| c.kind == ContractKind::GrpcProtoHash),
            "proto hash contract missing"
        );
    }

    #[test]
    fn test_dart_pbgrpc_consumer_extraction() {
        crate::isolate_test_data_dir();
        let dart = r#"
class AuthServiceClient extends $grpc.Client {
  static final _$login = $grpc.ClientMethod<$0.LoginRequest, $0.LoginResponse>(
      '/auth.AuthService/Login',
      ($0.LoginRequest value) => value.writeToBuffer(),
      $0.LoginResponse.fromBuffer);
}
"#;
        let mut out = Vec::new();
        extract_grpc_literals(
            "lib/gen/auth.pbgrpc.dart",
            dart,
            ContractRole::Consumer,
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "AuthService.Login");
        assert_eq!(out[0].role, ContractRole::Consumer);
    }

    #[test]
    fn test_go_generated_producer_and_consumer() {
        crate::isolate_test_data_dir();
        let go = r#"
func (c *authServiceClient) Login(ctx context.Context, in *LoginRequest, opts ...grpc.CallOption) (*LoginResponse, error) {
	err := c.cc.Invoke(ctx, "/auth.AuthService/Login", in, out, opts...)
}
func _AuthService_Login_Handler(srv interface{}, ctx context.Context, dec func(interface{}) error, interceptor grpc.UnaryServerInterceptor) (interface{}, error) {
	info := &grpc.UnaryServerInfo{
		FullMethod: "/auth.AuthService/Login",
	}
}
"#;
        let mut out = Vec::new();
        extract_go_grpc_generated("gen/auth_grpc.pb.go", go, &mut out);
        assert!(
            out.iter()
                .any(|c| c.role == ContractRole::Consumer && c.key == "AuthService.Login")
        );
        assert!(
            out.iter()
                .any(|c| c.role == ContractRole::Producer && c.key == "AuthService.Login")
        );
    }

    #[test]
    fn test_grpc_cross_link_flutter_to_go() {
        crate::isolate_test_data_dir();
        // Flutter consumes via generated stub; Go backend produces.
        let flutter = vec![contract(
            ContractKind::Grpc,
            ContractRole::Consumer,
            "AuthService.Login",
        )];
        let backend = vec![contract(
            ContractKind::Grpc,
            ContractRole::Producer,
            "AuthService.Login",
        )];
        let links = contract_links(&flutter, &backend);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].via, "grpc: AuthService.Login");
        assert_eq!(links[0].confidence, "high");
    }

    #[test]
    fn test_rest_exact_match() {
        crate::isolate_test_data_dir();
        let next = vec![contract(
            ContractKind::Rest,
            ContractRole::Consumer,
            "POST /api/v1/orders",
        )];
        let backend = vec![contract(
            ContractKind::Rest,
            ContractRole::Producer,
            "POST /api/v1/orders",
        )];
        let links = contract_links(&next, &backend);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].via, "rest: POST /api/v1/orders");
        assert_eq!(links[0].confidence, "high");
    }

    #[test]
    fn test_rest_param_normalized_match_is_medium() {
        crate::isolate_test_data_dir();
        let consumer = vec![contract(
            ContractKind::Rest,
            ContractRole::Consumer,
            "GET /api/users/42",
        )];
        let producer = vec![contract(
            ContractKind::Rest,
            ContractRole::Producer,
            "GET /api/users/{param}",
        )];
        let links = contract_links(&consumer, &producer);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].confidence, "medium");
    }

    #[test]
    fn test_rest_no_match_on_all_params() {
        crate::isolate_test_data_dir();
        // `/{param}` must not match everything.
        assert!(!rest_keys_match_normalized("GET /{param}", "GET /{param}"));
        assert!(!rest_keys_match_normalized("GET /a/b", "POST /a/b"));
    }

    #[test]
    fn test_normalize_rest_path() {
        crate::isolate_test_data_dir();
        assert_eq!(
            normalize_rest_path("/users/:id").as_deref(),
            Some("/users/{param}")
        );
        assert_eq!(
            normalize_rest_path("/users/{id}/orders").as_deref(),
            Some("/users/{param}/orders")
        );
        assert_eq!(
            normalize_rest_path("/users/${user.id}?full=1").as_deref(),
            Some("/users/{param}")
        );
        assert_eq!(
            normalize_rest_path("https://api.example.com/v1/items").as_deref(),
            Some("/v1/items")
        );
        // Map keys / non-paths are rejected.
        assert_eq!(normalize_rest_path("cache-key"), None);
        assert_eq!(normalize_rest_path("/"), None);
    }

    #[test]
    fn test_dio_consumer_extraction() {
        crate::isolate_test_data_dir();
        let dart = r#"
class OrdersApi {
  final Dio _dio;
  Future<Order> create(Order o) async {
    final res = await _dio.post('/api/v1/orders', data: o.toJson());
    return Order.fromJson(res.data);
  }
  Future<Order> byId(String id) async {
    final res = await _dio.get('/api/v1/orders/$id');
    return Order.fromJson(res.data);
  }
}
"#;
        let mut out = Vec::new();
        extract_rest_consumers("lib/api/orders.dart", dart, &mut out);
        assert!(
            out.iter().any(|c| c.key == "POST /api/v1/orders"),
            "got {:?}",
            out.iter().map(|c| &c.key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_fetch_consumer_with_method() {
        crate::isolate_test_data_dir();
        let ts = r#"
const res = await fetch(`/api/v1/orders/${id}`, {
  method: 'DELETE',
});
const list = await fetch('/api/v1/orders');
"#;
        let mut out = Vec::new();
        extract_rest_consumers("src/lib/api.ts", ts, &mut out);
        assert!(
            out.iter().any(|c| c.key == "DELETE /api/v1/orders/{param}"),
            "got {:?}",
            out.iter().map(|c| &c.key).collect::<Vec<_>>()
        );
        assert!(out.iter().any(|c| c.key == "GET /api/v1/orders"));
    }

    #[test]
    fn test_map_get_is_not_a_consumer() {
        crate::isolate_test_data_dir();
        let ts = "const v = cache.get('user-name');";
        let mut out = Vec::new();
        extract_rest_consumers("src/x.ts", ts, &mut out);
        assert!(out.is_empty(), "got {:?}", out);
    }

    #[test]
    fn test_next_app_router_producer() {
        crate::isolate_test_data_dir();
        let content = "export async function GET(req: Request) {}\nexport async function POST(req: Request) {}\n";
        let mut out = Vec::new();
        extract_next_route("app/api/orders/[id]/route.ts", content, &mut out);
        let keys: Vec<&str> = out.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"GET /api/orders/{param}"), "got {:?}", keys);
        assert!(keys.contains(&"POST /api/orders/{param}"), "got {:?}", keys);
    }

    #[test]
    fn test_go_gin_and_handlefunc_producers() {
        crate::isolate_test_data_dir();
        let go = r#"
func routes(r *gin.Engine) {
	r.GET("/api/v1/orders/:id", getOrder)
	r.POST("/api/v1/orders", createOrder)
	http.HandleFunc("/healthz", health)
}
"#;
        let mut out = Vec::new();
        extract_go_routes("internal/http/routes.go", go, &mut out);
        let keys: Vec<&str> = out.iter().map(|c| c.key.as_str()).collect();
        assert!(
            keys.contains(&"GET /api/v1/orders/{param}"),
            "got {:?}",
            keys
        );
        assert!(keys.contains(&"POST /api/v1/orders"), "got {:?}", keys);
        assert!(keys.contains(&"GET /healthz"), "got {:?}", keys);
    }

    #[test]
    fn test_express_route_producer_extraction() {
        crate::isolate_test_data_dir();
        let js = r#"
const app = express();
app.get('/api/items', (req, res) => res.json([]));
router.post('/api/items/:id', (req, res) => res.sendStatus(200));
"#;
        let mut out = Vec::new();
        extract_express_routes("src/routes.js", js, &mut out);
        let keys: Vec<&str> = out.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"GET /api/items"), "got {:?}", keys);
        assert!(keys.contains(&"POST /api/items/{param}"), "got {:?}", keys);
        assert!(out.iter().all(|c| c.role == ContractRole::Producer));
    }

    #[test]
    fn test_python_fastapi_and_flask_routes() {
        crate::isolate_test_data_dir();
        let py = r#"
@app.get("/users")
def list_users():
    pass

@router.post("/users/{id}/orders")
def create(id: str):
    pass

@app.route("/legacy", methods=["POST"])
def legacy():
    pass
"#;
        let mut out = Vec::new();
        extract_python_routes("api/routes.py", py, &mut out);
        let keys: Vec<&str> = out.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"GET /users"), "got {:?}", keys);
        assert!(
            keys.contains(&"POST /users/{param}/orders"),
            "got {:?}",
            keys
        );
        // @app.route with methods=[...] resolves the method.
        assert!(keys.contains(&"POST /legacy"), "got {:?}", keys);
    }

    #[test]
    fn test_python_route_defaults_to_get_without_methods() {
        crate::isolate_test_data_dir();
        // @app.route without a methods=[...] kwarg falls back to GET.
        let py = "@app.route(\"/health\")\ndef health():\n    pass\n";
        let mut out = Vec::new();
        extract_python_routes("api/health.py", py, &mut out);
        assert!(out.iter().any(|c| c.key == "GET /health"), "got {:?}", out);
    }

    #[test]
    fn test_dispatch_across_file_types_via_project_contracts() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("server.js"),
            "app.get('/api/items', (req, res) => res.json([]));\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("api.py"),
            "@app.get(\"/users\")\ndef u():\n    pass\n",
        )
        .unwrap();

        let project = Project {
            name: format!("contracts-dispatch-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let contracts = project_contracts(&project);
        let keys: Vec<&str> = contracts.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"GET /api/items"), "got {:?}", keys);
        assert!(keys.contains(&"GET /users"), "got {:?}", keys);

        let _ = std::fs::remove_file(ContractCache::path(&project));
    }

    #[test]
    fn test_proto_hash_vendored_copy_links() {
        crate::isolate_test_data_dir();
        // Two projects vendoring the same .proto (identical content hash) link
        // purely on key equality, regardless of role.
        let a = vec![contract(
            ContractKind::GrpcProtoHash,
            ContractRole::Producer,
            "sha256:abcdef012345",
        )];
        let b = vec![contract(
            ContractKind::GrpcProtoHash,
            ContractRole::Producer,
            "sha256:abcdef012345",
        )];
        let links = contract_links(&a, &b);
        assert_eq!(links.len(), 1);
        assert!(links[0].via.contains("shared definition"), "{:?}", links[0]);
        assert_eq!(links[0].confidence, "high");
    }

    #[test]
    fn test_rest_keys_match_normalized_edge_cases() {
        crate::isolate_test_data_dir();
        // Missing "METHOD path" split on either side → no match.
        assert!(!rest_keys_match_normalized("noSpaceHere", "GET /a"));
        assert!(!rest_keys_match_normalized("GET /a", "noSpaceHere"));
        // Different segment counts → no match.
        assert!(!rest_keys_match_normalized("GET /a/b", "GET /a"));
        // A param segment aligned with an agreeing concrete segment → match.
        assert!(rest_keys_match_normalized(
            "GET /users/{param}",
            "GET /users/42"
        ));
    }

    #[test]
    fn test_cache_invalidation_on_file_change() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("proto")).unwrap();
        let proto_path = dir.path().join("proto/auth.proto");
        std::fs::write(
            &proto_path,
            "service AuthService {\n  rpc Login(A) returns (B);\n}\n",
        )
        .unwrap();

        let project = Project {
            name: format!("contracts-test-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        let first = project_contracts(&project);
        assert!(first.iter().any(|c| c.key == "AuthService.Login"));

        // Change the file: the cache entry must be invalidated by hash.
        std::fs::write(
            &proto_path,
            "service AuthService {\n  rpc LoginV2(A) returns (B);\n}\n",
        )
        .unwrap();
        let second = project_contracts(&project);
        assert!(
            second.iter().any(|c| c.key == "AuthService.LoginV2"),
            "got {:?}",
            second.iter().map(|c| &c.key).collect::<Vec<_>>()
        );
        assert!(!second.iter().any(|c| c.key == "AuthService.Login"));

        // Cleanup the sidecar cache under the user data dir.
        let _ = std::fs::remove_file(ContractCache::path(&project));
    }
}
