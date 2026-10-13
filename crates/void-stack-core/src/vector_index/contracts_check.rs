//! `void contracts check`: fail when a consumed contract drifted.
//!
//! Verification mode over the existing gRPC/REST contract detection: for
//! every contract a project CONSUMES, look for a producer among the
//! registered projects. A consumed RPC whose service exists but no longer
//! exposes that method, or a REST route whose path exists under a
//! different method/signature, is a VIOLATION (non-zero exit in the CLI).
//! Consumed contracts with no related producer anywhere are reported as
//! external (Stripe, GitHub...) and never fail the check.
//!
//! Since 0.35 a REST call can also be checked against the project it is
//! *aimed at*: hosts map to registered projects (a project named like a
//! domain, `SITE_URL`-style keys in its `.env*`, `CNAME`, package.json
//! `homepage`, or `<global dir>/contracts.toml` `[hosts]`). A call to a
//! mapped host whose path the target does not serve is a violation
//! (`route not found`), a call that only differs by an API version prefix
//! (`/api/x` vs `/api/v1/x`) names the likely fix, and a target with no
//! routes at all is reported as unverified instead of silently external.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::global_config::GlobalConfig;
use crate::model::Project;

use super::contracts::{ApiContract, ContractKind, ContractRole, project_contracts};

#[derive(Debug, Clone, Serialize)]
pub struct ContractViolation {
    pub consumer_project: String,
    /// Consumer call site, `file:line`.
    pub consumer_site: String,
    /// The consumed contract key (`AuthService.Login`, `GET /api/v1/x`).
    pub contract: String,
    pub kind: ContractKind,
    /// The producer that owns the related service/route.
    pub producer_project: String,
    /// What drifted, human-readable.
    pub what_changed: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContractCheckReport {
    pub project: String,
    pub consumed: usize,
    pub matched: usize,
    /// Consumed contracts with no related producer in the registry —
    /// external APIs, informational only.
    pub external: Vec<String>,
    pub violations: Vec<ContractViolation>,
    /// Calls aimed at a registered project whose routes could not be
    /// checked (no REST routes found there).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unverified: Vec<String>,
    /// Consumed RPCs whose only "producer" is the consumer's own copy of
    /// the .proto (a Flutter app vendoring the backend's protos): not
    /// verified against anything, so not counted as matched.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub self_only: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Where REST calls go: host → registered project, plus the projects a
/// consumer's relative paths are resolved against (its base URLs).
#[derive(Debug, Clone, Default)]
pub struct Routing {
    /// (host, project) — a host also matches its subdomains.
    pub hosts: Vec<(String, String)>,
    /// Projects the consumer's API base URLs point to by exact host
    /// (relative calls are checked against them).
    pub default_targets: Vec<String>,
    /// Projects its API base URLs reach only through a parent domain
    /// (`api.shop.com` → project `shop.com`): too weak to fail on, reported
    /// as unverified.
    pub weak_targets: Vec<String>,
    /// Base-URL variables of the consumer → raw value (`baseUrl` →
    /// `${Env.api}/api`, `API_URL` → `https://x.app/api/v1`): a call
    /// written `$baseUrl/users` really goes to `/api/users`.
    pub base_values: std::collections::HashMap<String, String>,
}

impl Routing {
    /// Registered project serving `host` (exact or parent domain).
    pub fn project_for_host(&self, host: &str) -> Option<&str> {
        self.match_host(host).map(|(p, _)| p)
    }

    /// Like [`Self::project_for_host`], plus whether the host matched
    /// exactly (`true`) or only through a parent domain.
    pub fn match_host(&self, host: &str) -> Option<(&str, bool)> {
        let h = host.to_ascii_lowercase();
        if is_third_party_host(&h) {
            return None;
        }
        self.hosts
            .iter()
            .filter(|(known, _)| h == *known || h.ends_with(&format!(".{known}")))
            .max_by_key(|(known, _)| known.len())
            .map(|(known, p)| (p.as_str(), h == *known))
    }
}

/// Hosts that belong to code hosts and SaaS platforms, never to one of the
/// user's projects (a package.json `homepage` on GitHub must not make a
/// project "serve" github.com).
fn is_third_party_host(h: &str) -> bool {
    const HOSTS: &[&str] = &[
        "github.com",
        "githubusercontent.com",
        "gitlab.com",
        "bitbucket.org",
        "npmjs.com",
        "npmjs.org",
        "pypi.org",
        "crates.io",
        "vercel.app",
        "vercel.com",
        "netlify.app",
        "supabase.co",
        "supabase.com",
        "googleapis.com",
        "google.com",
        "firebaseapp.com",
        "firebaseio.com",
        "herokuapp.com",
        "amazonaws.com",
        "cloudfront.net",
        "azurewebsites.net",
        "stripe.com",
        "paypal.com",
        "cloudflare.com",
        "workers.dev",
        "pages.dev",
        "fly.dev",
        "render.com",
        "railway.app",
        "apple.com",
        "microsoft.com",
        "openai.com",
        "anthropic.com",
    ];
    HOSTS
        .iter()
        .any(|t| h == *t || h.ends_with(&format!(".{t}")))
}

/// Check one project's consumed contracts against every registered
/// producer. Scans all projects' contracts (cached per file SHA).
pub fn check_contracts(config: &GlobalConfig, project: &Project) -> ContractCheckReport {
    let consumed: Vec<ApiContract> = project_contracts(project)
        .into_iter()
        .filter(|c| c.role == ContractRole::Consumer)
        .collect();

    // Every registered project is scanned; they are independent, so in
    // parallel (dozens of projects, some on slow WSL / network paths).
    use rayon::prelude::*;
    let producers: Vec<(String, ApiContract)> = config
        .projects
        .par_iter()
        .filter(|p| std::path::Path::new(&crate::runner::local::strip_win_prefix(&p.path)).exists())
        .flat_map_iter(|p| {
            project_contracts(p)
                .into_iter()
                .filter(|c| c.role == ContractRole::Producer)
                .map(|c| (p.name.clone(), c))
                .collect::<Vec<_>>()
        })
        .collect();

    let routing = routing_for(config, project);
    let mut consumed = consumed;
    apply_base_prefixes(&mut consumed, &routing.base_values);
    find_violations_routed(&project.name, &consumed, &producers, &routing)
}

// ── Base-URL prefixes ───────────────────────────────────────

/// `get baseUrl => '…'`, `static const apiUrl = '…'`, `const API_BASE =
/// "…"`, `baseURL: '…'`: a name that reads like a base URL and a quoted
/// value that looks like one.
static BASE_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:\bget\s+|\b(?:const|final|var|let|static)\s+(?:\w+\s+)?|^\s*|[{,]\s*)(\w*(?i:url|base|api|host|endpoint)\w*)\s*(?:=>|=|:)\s*['"`]((?:\$\{[^}]*\}|[^'"`\s])+)['"`]"#,
    )
    .unwrap()
});

/// Collect base-URL definitions from source text and `KEY=value` env lines.
fn collect_base_defs(
    text: &str,
    env_file: bool,
    out: &mut std::collections::HashMap<String, String>,
) {
    for line in text.lines() {
        if env_file {
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim().trim_start_matches("export ").trim();
                let v = v.trim().trim_matches(['"', '\'']);
                if !k.is_empty() && !k.starts_with('#') && looks_like_base(v) {
                    out.entry(k.to_string()).or_insert_with(|| v.to_string());
                }
            }
            continue;
        }
        for c in BASE_DEF_RE.captures_iter(line) {
            let v = &c[2];
            if looks_like_base(v) {
                out.entry(c[1].to_string()).or_insert_with(|| v.to_string());
            }
        }
    }
}

fn looks_like_base(v: &str) -> bool {
    v.starts_with("http://")
        || v.starts_with("https://")
        || v.starts_with('$')
        || v.starts_with('/')
}

/// Variable a raw call target interpolates first: `${AppConfig.baseUrl}/x`
/// → `baseUrl`, `${dotenv.env['API_URL']}/x` → `API_URL`, `$api/x` → `api`.
fn base_var(raw: &str) -> Option<String> {
    let r = raw.trim();
    let inner = if let Some(rest) = r.strip_prefix("${") {
        &rest[..rest.find('}')?]
    } else {
        let rest = r.strip_prefix('$')?;
        &rest[..rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len())]
    };
    if let Some(q) = inner.find(['\'', '"']) {
        let after = &inner[q + 1..];
        return after.find(['\'', '"']).map(|e| after[..e].to_string());
    }
    inner
        .rsplit('.')
        .next()
        .map(|s| {
            s.trim_end_matches(|c: char| !(c.is_alphanumeric() || c == '_'))
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

/// Path prefix a base value adds (`https://x.app/api/` → `/api`), resolving
/// a leading `${other}` through `defs` (two levels at most).
fn base_prefix(value: &str, defs: &std::collections::HashMap<String, String>, depth: u8) -> String {
    let v = value.trim();
    let (head, rest) = if v.starts_with('$') {
        let cut = if v.starts_with("${") {
            v.find('}').map(|i| i + 1).unwrap_or(v.len())
        } else {
            v.find('/').unwrap_or(v.len())
        };
        let head = if depth < 2 {
            base_var(&v[..cut])
                .and_then(|n| defs.get(&n))
                .map(|d| base_prefix(d, defs, depth + 1))
                .unwrap_or_default()
        } else {
            String::new()
        };
        (head, &v[cut..])
    } else if let Some(after) = v
        .strip_prefix("http://")
        .or_else(|| v.strip_prefix("https://"))
    {
        (
            String::new(),
            after.find('/').map(|i| &after[i..]).unwrap_or(""),
        )
    } else {
        (String::new(), v)
    };
    let rest = rest
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let rest = if rest.starts_with('/') { rest } else { "" };
    format!("{head}{rest}")
}

/// Rewrite consumed REST keys whose call goes through a base-URL variable
/// with a path (`$baseUrl/marketplace` with `baseUrl = '$host/api'` →
/// `GET /api/marketplace`). Without this, `/api/marketplace` vs a served
/// `/api/v1/marketplace` was never compared.
pub fn apply_base_prefixes(
    consumed: &mut [ApiContract],
    defs: &std::collections::HashMap<String, String>,
) {
    if defs.is_empty() {
        return;
    }
    for c in consumed
        .iter_mut()
        .filter(|c| c.kind == ContractKind::Rest && c.detail.trim_start().starts_with('$'))
    {
        // A SaaS base (`${SUPABASE_URL}/rest/v1/x`) stays external.
        if third_party_base(&c.detail) {
            continue;
        }
        let Some(var) = base_var(&c.detail) else {
            continue;
        };
        let Some(value) = defs.get(&var) else {
            continue;
        };
        let prefix = base_prefix(value, defs, 0);
        // Only literal path prefixes (`/api`, `/api/v1`): an unresolved
        // interpolation would become `{param}` segments that match anything.
        if prefix.is_empty() || prefix.contains(['$', '{', '}']) {
            continue;
        }
        let Some((method, path)) = c.key.split_once(' ') else {
            continue;
        };
        if path == prefix || path.starts_with(&format!("{prefix}/")) {
            continue;
        }
        if let Some(p) = super::contracts::normalize_rest_path(&format!("{prefix}{path}")) {
            c.key = format!("{method} {p}");
        }
    }
}

/// Base-URL definitions of a project (env files + source), for callers
/// that don't build a full [`Routing`].
pub fn base_definitions(project: &Project) -> std::collections::HashMap<String, String> {
    let root = std::path::PathBuf::from(crate::runner::local::strip_win_prefix(&project.path));
    let mut defs = std::collections::HashMap::new();
    for f in ENV_FILES {
        if let Ok(text) = std::fs::read_to_string(root.join(f)) {
            collect_base_defs(&text, true, &mut defs);
        }
    }
    for rel in super::contracts::candidate_files(&root) {
        if let Some(text) = crate::fs_util::read_source(&root.join(&rel)) {
            collect_base_defs(&text, false, &mut defs);
        }
    }
    defs
}

/// Pure matching core (unit-testable without a filesystem).
pub fn find_violations(
    consumer_project: &str,
    consumed: &[ApiContract],
    producers: &[(String, ApiContract)],
) -> ContractCheckReport {
    find_violations_routed(consumer_project, consumed, producers, &Routing::default())
}

/// [`find_violations`] with knowledge of which project each call targets.
pub fn find_violations_routed(
    consumer_project: &str,
    consumed: &[ApiContract],
    producers: &[(String, ApiContract)],
    routing: &Routing,
) -> ContractCheckReport {
    let mut unverified = Vec::new();
    let mut matched = 0usize;
    let mut self_only = 0usize;
    let mut external = Vec::new();
    let mut violations = Vec::new();

    // gRPC services some *other* project defines: for those, the
    // consumer's own copy of the proto is not a producer (it used to make
    // a Flutter app match 1407/1409 RPCs against itself).
    let is_self = |owner: &str| owner.eq_ignore_ascii_case(consumer_project);
    let service_of = |key: &str| key.split('.').next().unwrap_or(key).to_string();
    let foreign_services: std::collections::HashSet<String> = producers
        .iter()
        .filter(|(o, p)| p.kind == ContractKind::Grpc && !is_self(o))
        .map(|(_, p)| service_of(&p.key))
        .collect();
    let own_copy_only: Vec<(String, ApiContract)>;
    let producers: &[(String, ApiContract)] = {
        own_copy_only = producers
            .iter()
            .filter(|(o, p)| {
                !(p.kind == ContractKind::Grpc
                    && is_self(o)
                    && foreign_services.contains(&service_of(&p.key)))
            })
            .cloned()
            .collect();
        &own_copy_only
    };

    for c in consumed {
        if c.kind == ContractKind::GrpcProtoHash {
            // Vendored proto copies always "match" themselves; drift shows
            // up through the Grpc-kind keys instead.
            continue;
        }
        // Exact (or param-compatible REST) match with any producer.
        let exact = producers.iter().any(|(_, p)| {
            p.kind == c.kind
                && match c.kind {
                    ContractKind::Rest => rest_keys_match(&c.key, &p.key),
                    _ => p.key == c.key,
                }
        });
        if exact {
            let only_self = c.kind == ContractKind::Grpc
                && producers
                    .iter()
                    .filter(|(_, p)| p.kind == c.kind && p.key == c.key)
                    .all(|(o, _)| is_self(o));
            if only_self {
                self_only += 1;
            } else {
                matched += 1;
            }
            continue;
        }

        // No exact match: is there a RELATED producer (same gRPC service /
        // same REST path)? That's drift, not an external API.
        match c.kind {
            ContractKind::Grpc => {
                let service = c.key.split('.').next().unwrap_or(&c.key);
                let related: Vec<&(String, ApiContract)> = producers
                    .iter()
                    .filter(|(_, p)| {
                        p.kind == ContractKind::Grpc && p.key.split('.').next() == Some(service)
                    })
                    .collect();
                if let Some((owner, _)) = related.first() {
                    let available: Vec<&str> =
                        related.iter().map(|(_, p)| p.key.as_str()).collect();
                    violations.push(ContractViolation {
                        consumer_project: consumer_project.to_string(),
                        consumer_site: format!("{}:{}", c.file, c.line),
                        contract: c.key.clone(),
                        kind: c.kind,
                        producer_project: owner.clone(),
                        what_changed: format!(
                            "service '{}' no longer exposes this rpc (available: {})",
                            service,
                            available.join(", ")
                        ),
                    });
                } else {
                    external.push(c.key.clone());
                }
            }
            ContractKind::Rest => {
                let path = c.key.split_once(' ').map(|(_, p)| p).unwrap_or(&c.key);
                let related: Vec<&(String, ApiContract)> = producers
                    .iter()
                    .filter(|(_, p)| {
                        p.kind == ContractKind::Rest
                            && p.key
                                .split_once(' ')
                                .map(|(_, pp)| rest_paths_match(path, pp))
                                .unwrap_or(false)
                    })
                    .collect();
                if let Some((owner, _)) = related.first() {
                    let available: Vec<&str> =
                        related.iter().map(|(_, p)| p.key.as_str()).collect();
                    violations.push(ContractViolation {
                        consumer_project: consumer_project.to_string(),
                        consumer_site: format!("{}:{}", c.file, c.line),
                        contract: c.key.clone(),
                        kind: c.kind,
                        producer_project: owner.clone(),
                        what_changed: format!(
                            "route signature changed — producer exposes: {}",
                            available.join(", ")
                        ),
                    });
                } else if let Some((owner, near)) = version_prefix_match(path, producers) {
                    // Same route behind a different API version prefix —
                    // almost always the bug (/api/x called, /api/v1/x served).
                    violations.push(ContractViolation {
                        consumer_project: consumer_project.to_string(),
                        consumer_site: format!("{}:{}", c.file, c.line),
                        contract: c.key.clone(),
                        kind: c.kind,
                        producer_project: owner.to_string(),
                        what_changed: format!(
                            "route not found — {} serves {} (API version prefix differs)",
                            owner, near
                        ),
                    });
                } else {
                    let (targets, strong) = call_targets(c, consumer_project, routing);
                    let served: Vec<&str> = targets
                        .iter()
                        .copied()
                        .filter(|t| {
                            producers
                                .iter()
                                .any(|(o, p)| o == t && p.kind == ContractKind::Rest)
                        })
                        .collect();
                    let likely = likely_owner(path, producers, consumer_project, &targets)
                        .map(|(o, k)| format!("; likely served by {o} ({k})"))
                        .unwrap_or_default();
                    if let Some(target) = served.first().filter(|_| !strong) {
                        unverified.push(format!(
                            "{} → {}?: route not found there (target inferred from a parent domain; map the host in contracts.toml to check it){likely}",
                            c.key, target
                        ));
                    } else if let Some(target) = served.first() {
                        let hint = closest_route(path, target, producers)
                            .map(|k| format!("; closest: {k}"))
                            .unwrap_or_default();
                        violations.push(ContractViolation {
                            consumer_project: consumer_project.to_string(),
                            consumer_site: format!("{}:{}", c.file, c.line),
                            contract: c.key.clone(),
                            kind: c.kind,
                            producer_project: target.to_string(),
                            what_changed: format!("route not found in {target}{hint}"),
                        });
                    } else if let Some(target) = targets.first() {
                        unverified.push(format!(
                            "{} → {}: no REST routes found there{likely}",
                            c.key, target
                        ));
                    } else {
                        external.push(c.key.clone());
                    }
                }
            }
            ContractKind::GrpcProtoHash => unreachable!("filtered above"),
        }
    }

    external.sort();
    external.dedup();
    unverified.sort();
    unverified.dedup();
    ContractCheckReport {
        project: consumer_project.to_string(),
        consumed: consumed
            .iter()
            .filter(|c| c.kind != ContractKind::GrpcProtoHash)
            .count(),
        matched,
        external,
        violations,
        unverified,
        self_only,
    }
}

/// The project (other than the consumer and the guessed targets) whose
/// REST routes share the longest leading path with `path` — at least two
/// non-version segments, e.g. `/referrals/track-click` vs
/// `/api/v1/referrals/{param}`. Points an unverified call at the service
/// that most likely owns it instead of the domain it was guessed from.
fn likely_owner<'a>(
    path: &str,
    producers: &'a [(String, ApiContract)],
    consumer: &str,
    guessed: &[&str],
) -> Option<(&'a str, &'a str)> {
    let segs = |p: &str| -> Vec<String> {
        p.split('/')
            .filter(|s| !s.is_empty() && !is_version_segment(s) && *s != "api")
            .map(|s| s.to_string())
            .collect()
    };
    let want = segs(path);
    let mut best: Option<(usize, &str, &str)> = None;
    for (owner, p) in producers {
        if p.kind != ContractKind::Rest
            || owner.eq_ignore_ascii_case(consumer)
            || guessed.iter().any(|g| g.eq_ignore_ascii_case(owner))
        {
            continue;
        }
        let Some((_, pp)) = p.key.split_once(' ') else {
            continue;
        };
        let have = segs(pp);
        let shared = want
            .iter()
            .zip(have.iter())
            .take_while(|(a, b)| a == b || *b == "{param}" || *a == "{param}")
            .count();
        // The whole call path (at least two segments) or three leading
        // segments: `users/me` alone is shared by unrelated APIs.
        let enough = shared >= 3 || (shared >= 2 && shared == want.len());
        if enough && best.is_none_or(|(n, _, _)| shared > n) {
            best = Some((shared, owner.as_str(), p.key.as_str()));
        }
    }
    best.map(|(_, o, k)| (o, k))
}

/// Projects a consumed REST call is aimed at: the host of an absolute URL
/// if it maps to a registered project, else the consumer's base-URL
/// targets. Never the consumer itself.
/// `strong` is false when the target is only inferred through a parent
/// domain: such calls are reported as unverified, never as violations.
fn call_targets<'a>(c: &ApiContract, consumer: &str, routing: &'a Routing) -> (Vec<&'a str>, bool) {
    if third_party_base(&c.detail) {
        return (Vec::new(), false);
    }
    let (targets, strong): (Vec<&str>, bool) = match url_host(&c.detail) {
        Some(host) => match routing.match_host(&host) {
            Some((p, exact)) => (vec![p], exact),
            None => (Vec::new(), false),
        },
        None if !routing.default_targets.is_empty() => (
            routing.default_targets.iter().map(|s| s.as_str()).collect(),
            true,
        ),
        None => (
            routing.weak_targets.iter().map(|s| s.as_str()).collect(),
            false,
        ),
    };
    let targets = targets
        .into_iter()
        .filter(|t| !t.eq_ignore_ascii_case(consumer))
        .collect();
    (targets, strong)
}

/// A call through an interpolated base that names a SaaS
/// (`${SUPABASE_URL}/rest/v1/users`, `$stripeApi/v1/charges`): external.
fn third_party_base(raw: &str) -> bool {
    let r = raw.trim();
    if !r.starts_with('$') {
        return false;
    }
    let var = r.split('/').next().unwrap_or("").to_ascii_lowercase();
    [
        "supabase",
        "stripe",
        "github",
        "google",
        "firebase",
        "openai",
        "anthropic",
        "paypal",
        "sentry",
        "cloudinary",
        "twilio",
        "sendgrid",
        "resend",
        "mapbox",
        "algolia",
        "posthog",
    ]
    .iter()
    .any(|k| var.contains(k))
}

/// Host of an absolute `http(s)://` URL.
pub(crate) fn url_host(raw: &str) -> Option<String> {
    let rest = raw
        .trim()
        .strip_prefix("https://")
        .or_else(|| raw.trim().strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?.split(':').next()?;
    (!host.is_empty() && host.contains('.')).then(|| host.to_ascii_lowercase())
}

fn strip_version(path: &str) -> String {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !is_version_segment(s))
        .collect::<Vec<_>>()
        .join("/")
}

fn is_version_segment(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.len() >= 2 && s.starts_with('v') && s[1..].chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// A producer route equal to `path` once API version segments are
/// ignored on both sides (but not equal with them).
fn version_prefix_match<'a>(
    path: &str,
    producers: &'a [(String, ApiContract)],
) -> Option<(&'a str, &'a str)> {
    let want = strip_version(path);
    if want.is_empty() {
        return None;
    }
    producers.iter().find_map(|(owner, p)| {
        if p.kind != ContractKind::Rest {
            return None;
        }
        let pp = p.key.split_once(' ').map(|(_, x)| x).unwrap_or(&p.key);
        (rest_paths_match(&strip_version(pp), &want) && !rest_paths_match(pp, path))
            .then_some((owner.as_str(), p.key.as_str()))
    })
}

/// The target's route sharing the longest path suffix with `path` (≥ 1
/// trailing segment besides params).
fn closest_route<'a>(
    path: &str,
    target: &str,
    producers: &'a [(String, ApiContract)],
) -> Option<&'a str> {
    let want: Vec<&str> = path.trim_matches('/').split('/').collect();
    producers
        .iter()
        .filter(|(o, p)| o == target && p.kind == ContractKind::Rest)
        .map(|(_, p)| {
            let pp = p.key.split_once(' ').map(|(_, x)| x).unwrap_or(&p.key);
            let segs: Vec<&str> = pp.trim_matches('/').split('/').collect();
            let common = want
                .iter()
                .rev()
                .zip(segs.iter().rev())
                .take_while(|(a, b)| a == b && **a != "{param}")
                .count();
            (common, p.key.as_str())
        })
        .filter(|(n, _)| *n >= 1)
        .max_by_key(|(n, _)| *n)
        .map(|(_, k)| k)
}

// ── Routing discovery ───────────────────────────────────────

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://([A-Za-z0-9][A-Za-z0-9.\-]*\.[A-Za-z]{2,})"#).unwrap());

/// Env keys that name the project's OWN public URL (not an API it calls).
fn is_self_url_key(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    !k.contains("API")
        && (k.ends_with("SITE_URL")
            || k.ends_with("APP_URL")
            || k == "NEXTAUTH_URL"
            || k == "PUBLIC_URL"
            || k.ends_with("_DOMAIN")
            || k == "DOMAIN"
            || k.ends_with("BASE_URL") && k.contains("PUBLIC"))
}

const ENV_FILES: &[&str] = &[
    ".env",
    ".env.example",
    ".env.local",
    ".env.production",
    ".env.sample",
];

/// Hosts a project serves, from its name, env files, CNAME and
/// package.json `homepage`.
pub fn project_self_hosts(project: &Project) -> Vec<String> {
    let root = std::path::PathBuf::from(crate::runner::local::strip_win_prefix(&project.path));
    let mut hosts = BTreeSet::new();
    let name = project.name.to_ascii_lowercase();
    if looks_like_domain(&name) {
        hosts.insert(name);
    }
    for f in ENV_FILES {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            if !is_self_url_key(k.trim()) {
                continue;
            }
            let v = v.trim().trim_matches(['"', '\'']);
            if let Some(h) =
                url_host(v).or_else(|| looks_like_domain(v).then(|| v.to_ascii_lowercase()))
            {
                hosts.insert(h);
            }
        }
    }
    if let Ok(cname) = std::fs::read_to_string(root.join("CNAME")) {
        let c = cname.trim().to_ascii_lowercase();
        if looks_like_domain(&c) {
            hosts.insert(c);
        }
    }
    if let Ok(pkg) = std::fs::read_to_string(root.join("package.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&pkg)
        && let Some(h) = v
            .get("homepage")
            .and_then(|h| h.as_str())
            .and_then(url_host)
    {
        hosts.insert(h);
    }
    hosts
        .into_iter()
        .filter(|h| {
            !h.starts_with("localhost") && !h.ends_with(".local") && !h.ends_with("vercel.app")
        })
        .collect()
}

fn looks_like_domain(s: &str) -> bool {
    const TLDS: &[&str] = &[
        "com", "app", "store", "dev", "io", "net", "org", "co", "ai", "shop", "site", "online",
        "tech", "cloud", "me", "xyz", "es", "ve", "mx", "ar",
    ];
    let s = s.trim().to_ascii_lowercase();
    !s.contains('/')
        && !s.contains(' ')
        && s.split('.').count() >= 2
        && s.rsplit('.').next().is_some_and(|tld| TLDS.contains(&tld))
        && s.split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// User overrides: `<global dir>/contracts.toml` with
/// `[hosts]` `"api.example.com" = "backend-project"`.
fn configured_hosts() -> Vec<(String, String)> {
    let Ok(dir) = crate::global_config::global_config_dir() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(dir.join("contracts.toml")) else {
        return Vec::new();
    };
    let Ok(v) = toml::from_str::<toml::Value>(&text) else {
        return Vec::new();
    };
    v.get("hosts")
        .and_then(|h| h.as_table())
        .map(|t| {
            t.iter()
                .filter_map(|(h, p)| p.as_str().map(|p| (h.to_ascii_lowercase(), p.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Build the routing for `consumer`: every registered project's hosts,
/// and which of them the consumer's base URLs point to.
pub fn routing_for(config: &GlobalConfig, consumer: &Project) -> Routing {
    let mut hosts = configured_hosts();
    for p in &config.projects {
        for h in project_self_hosts(p) {
            if !hosts.iter().any(|(known, _)| *known == h) {
                hosts.push((h, p.name.clone()));
            }
        }
    }
    let mut routing = Routing {
        hosts,
        default_targets: Vec::new(),
        weak_targets: Vec::new(),
        base_values: Default::default(),
    };
    // Hosts the consumer uses as an API base: env keys and source lines
    // that talk about an API/base URL (a marketing link to another site is
    // not where its relative calls go).
    let root = std::path::PathBuf::from(crate::runner::local::strip_win_prefix(&consumer.path));
    let mut strong: BTreeSet<String> = BTreeSet::new();
    let mut weak: BTreeSet<String> = BTreeSet::new();
    let mut scan = |text: &str| {
        for line in text.lines().filter(|l| is_api_base_line(l)) {
            for cap in URL_RE.captures_iter(line) {
                if let Some((p, exact)) = routing.match_host(&cap[1])
                    && !p.eq_ignore_ascii_case(&consumer.name)
                {
                    if exact {
                        strong.insert(p.to_string());
                    } else {
                        weak.insert(p.to_string());
                    }
                }
            }
        }
    };
    let mut defs = std::collections::HashMap::new();
    for f in ENV_FILES {
        if let Ok(text) = std::fs::read_to_string(root.join(f)) {
            scan(&text);
            collect_base_defs(&text, true, &mut defs);
        }
    }
    for rel in super::contracts::candidate_files(&root) {
        if let Some(text) = crate::fs_util::read_source(&root.join(&rel)) {
            scan(&text);
            collect_base_defs(&text, false, &mut defs);
        }
    }
    routing.base_values = defs;
    weak.retain(|p| !strong.contains(p));
    routing.default_targets = strong.into_iter().collect();
    routing.weak_targets = weak.into_iter().collect();
    routing
}

/// A line that defines where API calls go: `API_URL=…`, `baseUrl: …`,
/// `axios.create({ baseURL: … })`, `const BACKEND = …`.
fn is_api_base_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    [
        "api",
        "base_url",
        "baseurl",
        "backend",
        "endpoint",
        "server_url",
    ]
    .iter()
    .any(|k| l.contains(k))
}

/// `METHOD /path` keys match when methods are equal and paths are
/// param-compatible.
fn rest_keys_match(a: &str, b: &str) -> bool {
    match (a.split_once(' '), b.split_once(' ')) {
        (Some((ma, pa)), Some((mb, pb))) => {
            (ma.eq_ignore_ascii_case(mb) || ma == "ANY" || mb == "ANY") && rest_paths_match(pa, pb)
        }
        _ => a == b,
    }
}

/// Segment-wise path equality where `{param}` matches any literal segment
/// on the other side (either side may be the normalized one).
fn rest_paths_match(a: &str, b: &str) -> bool {
    let sa: Vec<&str> = a.trim_matches('/').split('/').collect();
    let sb: Vec<&str> = b.trim_matches('/').split('/').collect();
    sa.len() == sb.len()
        && sa
            .iter()
            .zip(&sb)
            .all(|(x, y)| x == y || *x == "{param}" || *y == "{param}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(kind: ContractKind, role: ContractRole, key: &str) -> ApiContract {
        ApiContract {
            kind,
            role,
            key: key.to_string(),
            detail: String::new(),
            file: "src/client.ts".into(),
            line: 42,
        }
    }

    fn producer(project: &str, kind: ContractKind, key: &str) -> (String, ApiContract) {
        (
            project.to_string(),
            contract(kind, ContractRole::Producer, key),
        )
    }

    #[test]
    fn test_exact_matches_pass() {
        let consumed = vec![
            contract(ContractKind::Grpc, ContractRole::Consumer, "Auth.Login"),
            contract(
                ContractKind::Rest,
                ContractRole::Consumer,
                "GET /api/users/{param}",
            ),
        ];
        let producers = vec![
            producer("backend", ContractKind::Grpc, "Auth.Login"),
            producer("backend", ContractKind::Rest, "GET /api/users/{param}"),
        ];
        let report = find_violations("front", &consumed, &producers);
        assert_eq!(report.matched, 2);
        assert!(report.violations.is_empty());
        assert!(report.external.is_empty());
    }

    #[test]
    fn test_removed_rpc_is_violation() {
        let consumed = vec![contract(
            ContractKind::Grpc,
            ContractRole::Consumer,
            "Auth.Refresh",
        )];
        let producers = vec![
            producer("backend", ContractKind::Grpc, "Auth.Login"),
            producer("backend", ContractKind::Grpc, "Auth.Logout"),
        ];
        let report = find_violations("front", &consumed, &producers);
        assert_eq!(report.violations.len(), 1);
        let v = &report.violations[0];
        assert_eq!(v.producer_project, "backend");
        assert_eq!(v.contract, "Auth.Refresh");
        assert!(v.what_changed.contains("no longer exposes"));
        assert!(v.what_changed.contains("Auth.Login"));
        assert_eq!(v.consumer_site, "src/client.ts:42");
    }

    #[test]
    fn test_rest_method_change_is_violation() {
        let consumed = vec![contract(
            ContractKind::Rest,
            ContractRole::Consumer,
            "PUT /api/orders/{param}",
        )];
        let producers = vec![producer(
            "backend",
            ContractKind::Rest,
            "PATCH /api/orders/{param}",
        )];
        let report = find_violations("front", &consumed, &producers);
        assert_eq!(report.violations.len(), 1);
        assert!(
            report.violations[0]
                .what_changed
                .contains("PATCH /api/orders/{param}")
        );
    }

    #[test]
    fn test_param_compatible_rest_matches() {
        // Consumer normalized a literal id; producer declares {param}.
        let consumed = vec![contract(
            ContractKind::Rest,
            ContractRole::Consumer,
            "GET /api/orders/123",
        )];
        let producers = vec![producer(
            "backend",
            ContractKind::Rest,
            "GET /api/orders/{param}",
        )];
        let report = find_violations("front", &consumed, &producers);
        assert_eq!(report.matched, 1);
        assert!(report.violations.is_empty());
    }

    fn routed(detail: &str, key: &str) -> ApiContract {
        let mut c = contract(ContractKind::Rest, ContractRole::Consumer, key);
        c.detail = detail.to_string();
        c
    }

    #[test]
    fn test_call_to_mapped_host_with_missing_route_is_violation() {
        let consumed = vec![routed(
            "https://iunci.app/api/analytics/batch",
            "POST /api/analytics/batch",
        )];
        let producers = vec![
            producer("iunci.app", ContractKind::Rest, "GET /api/users/{param}"),
            producer("iunci.app", ContractKind::Rest, "POST /api/events/batch"),
        ];
        let routing = Routing {
            hosts: vec![("iunci.app".into(), "iunci.app".into())],
            default_targets: vec![],
            weak_targets: vec![],
            base_values: Default::default(),
        };
        let report = find_violations_routed("iunci-flutter", &consumed, &producers, &routing);
        assert_eq!(report.violations.len(), 1, "{report:?}");
        let v = &report.violations[0];
        assert_eq!(v.producer_project, "iunci.app");
        assert!(v.what_changed.starts_with("route not found in iunci.app"));
        assert!(
            v.what_changed.contains("closest: POST /api/events/batch"),
            "{}",
            v.what_changed
        );
        // Without routing the same call was silently external.
        let plain = find_violations("iunci-flutter", &consumed, &producers);
        assert!(plain.violations.is_empty());
        assert_eq!(plain.external.len(), 1);
    }

    #[test]
    fn test_relative_call_uses_base_url_targets_and_subdomains() {
        let consumed = vec![routed(
            "/api/devices/register",
            "POST /api/devices/register",
        )];
        let producers = vec![producer("iunci.app", ContractKind::Rest, "GET /api/health")];
        let routing = Routing {
            hosts: vec![("iunci.app".into(), "iunci.app".into())],
            default_targets: vec!["iunci.app".into()],
            weak_targets: vec![],
            base_values: Default::default(),
        };
        let report = find_violations_routed("iunci-flutter", &consumed, &producers, &routing);
        assert_eq!(report.violations.len(), 1);
        assert_eq!(routing.project_for_host("api.iunci.app"), Some("iunci.app"));
        assert_eq!(routing.project_for_host("notiunci.app"), None);
    }

    #[test]
    fn test_version_prefix_mismatch_is_named() {
        let consumed = vec![routed(
            "/api/marketplace/items",
            "GET /api/marketplace/items",
        )];
        let producers = vec![producer(
            "iunci-backend",
            ContractKind::Rest,
            "GET /api/v1/marketplace/items",
        )];
        let report = find_violations("iunci.store", &consumed, &producers);
        assert_eq!(report.violations.len(), 1);
        let v = &report.violations[0];
        assert_eq!(v.producer_project, "iunci-backend");
        assert!(v.what_changed.contains("GET /api/v1/marketplace/items"));
        assert!(v.what_changed.contains("version prefix"));
    }

    #[test]
    fn test_parent_domain_target_is_unverified_not_violation() {
        // api.iunci.store reaches project iunci.store only by parent domain:
        // the API may well live in another project (the backend).
        let consumed = vec![routed("https://api.iunci.store/v1/plans", "GET /v1/plans")];
        let producers = vec![producer(
            "iunci.store",
            ContractKind::Rest,
            "GET /api/health",
        )];
        let routing = Routing {
            hosts: vec![("iunci.store".into(), "iunci.store".into())],
            default_targets: vec![],
            weak_targets: vec![],
            base_values: Default::default(),
        };
        let report = find_violations_routed("iunci.app", &consumed, &producers, &routing);
        assert!(report.violations.is_empty(), "{report:?}");
        assert_eq!(report.unverified.len(), 1);
        assert!(report.unverified[0].contains("iunci.store?"));
    }

    #[test]
    fn test_saas_base_variable_is_external() {
        let consumed = vec![routed(
            "${SUPABASE_URL}/rest/v1/users",
            "HEAD /rest/v1/users",
        )];
        let producers = vec![producer("shop.dev", ContractKind::Rest, "GET /api/x")];
        let routing = Routing {
            hosts: vec![("shop.dev".into(), "shop.dev".into())],
            default_targets: vec!["shop.dev".into()],
            weak_targets: vec![],
            base_values: Default::default(),
        };
        let report = find_violations_routed("app", &consumed, &producers, &routing);
        assert!(report.violations.is_empty());
        assert_eq!(report.external, vec!["HEAD /rest/v1/users"]);
    }

    #[test]
    fn test_third_party_hosts_never_map_to_projects() {
        let routing = Routing {
            hosts: vec![("github.com".into(), "rtc".into())],
            default_targets: vec![],
            weak_targets: vec![],
            base_values: Default::default(),
        };
        assert_eq!(routing.project_for_host("api.github.com"), None);
        assert!(is_api_base_line("const API_URL = 'https://x.dev'"));
        assert!(!is_api_base_line("<a href='https://x.dev'>shop</a>"));
    }

    #[test]
    fn test_target_without_routes_is_unverified() {
        let consumed = vec![routed("https://api.iunci.store/x/y", "GET /x/y")];
        let routing = Routing {
            hosts: vec![("iunci.store".into(), "iunci.store".into())],
            default_targets: vec![],
            weak_targets: vec![],
            base_values: Default::default(),
        };
        let report = find_violations_routed("app", &consumed, &[], &routing);
        assert!(report.violations.is_empty());
        assert!(report.external.is_empty());
        assert_eq!(
            report.unverified,
            vec!["GET /x/y → iunci.store: no REST routes found there"]
        );
    }

    #[test]
    fn test_own_proto_copy_is_not_a_match_when_another_project_serves_it() {
        let consumed = vec![
            contract(
                ContractKind::Grpc,
                ContractRole::Consumer,
                "AuthService.Login",
            ),
            contract(
                ContractKind::Grpc,
                ContractRole::Consumer,
                "AuthService.Logout",
            ),
            contract(
                ContractKind::Grpc,
                ContractRole::Consumer,
                "LocalService.Ping",
            ),
        ];
        let producers = vec![
            // the app's vendored copy of the backend proto
            producer("app", ContractKind::Grpc, "AuthService.Login"),
            producer("app", ContractKind::Grpc, "AuthService.Logout"),
            producer("app", ContractKind::Grpc, "LocalService.Ping"),
            // the backend no longer has Logout
            producer("backend", ContractKind::Grpc, "AuthService.Login"),
        ];
        let r = find_violations("app", &consumed, &producers);
        assert_eq!(r.matched, 1, "only Login is verified against backend");
        assert_eq!(
            r.self_only, 1,
            "LocalService only exists in the app's own proto"
        );
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].contract, "AuthService.Logout");
        assert_eq!(r.violations[0].producer_project, "backend");
    }

    #[test]
    fn test_base_url_getter_prefix_is_applied() {
        let mut defs = std::collections::HashMap::new();
        collect_base_defs("API_URL=https://iunci.app/api\n", true, &mut defs);
        collect_base_defs(
            "class Env {\n  static String get baseUrl => '${dotenv.env['API_URL']}';\n  static const plain = 'x';\n}\nfinal hostUrl = 'https://iunci.app';\n",
            false,
            &mut defs,
        );
        let mut consumed = vec![
            routed("${Env.baseUrl}/marketplace", "GET /marketplace"),
            routed("$hostUrl/health", "GET /health"),
            routed("/plain", "GET /plain"),
        ];
        apply_base_prefixes(&mut consumed, &defs);
        let keys: Vec<&str> = consumed.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["GET /api/marketplace", "GET /health", "GET /plain"]
        );

        // …which now meets the served /api/v1/marketplace as a version mismatch.
        let producers = vec![producer(
            "iunci.app",
            ContractKind::Rest,
            "GET /api/v1/marketplace",
        )];
        let r = find_violations("iunci-flutter", &consumed[..1], &producers);
        assert_eq!(r.violations.len(), 1);
        assert!(r.violations[0].what_changed.contains("/api/v1/marketplace"));
    }

    #[test]
    fn test_saas_and_unresolved_bases_keep_their_key() {
        let mut defs = std::collections::HashMap::new();
        collect_base_defs(
            "SUPABASE_URL=https://abc.supabase.co\nPROXY_URL=${OTHER}/${STAGE}\n",
            true,
            &mut defs,
        );
        let mut consumed = vec![
            routed("${SUPABASE_URL}/rest/v1/users", "HEAD /rest/v1/users"),
            routed("$PROXY_URL/items", "GET /items"),
        ];
        apply_base_prefixes(&mut consumed, &defs);
        let keys: Vec<&str> = consumed.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, vec!["HEAD /rest/v1/users", "GET /items"]);
    }

    #[test]
    fn test_likely_owner_needs_more_than_a_common_prefix() {
        let consumed = vec![routed(
            "https://api.iunci.store/api/v1/users/me/organizations",
            "GET /api/v1/users/me/organizations",
        )];
        let routing = Routing {
            hosts: vec![("iunci.store".into(), "iunci.store".into())],
            ..Default::default()
        };
        let producers = vec![producer(
            "VoiceStudio",
            ContractKind::Rest,
            "GET /users/me/items",
        )];
        let r = find_violations_routed("app", &consumed, &producers, &routing);
        assert!(
            !r.unverified[0].contains("likely served by"),
            "{:?}",
            r.unverified
        );
    }

    #[test]
    fn test_unverified_names_the_likely_owner() {
        let consumed = vec![routed(
            "https://api.iunci.store/referrals/track-click",
            "POST /referrals/track-click",
        )];
        let routing = Routing {
            hosts: vec![("iunci.store".into(), "iunci.store".into())],
            ..Default::default()
        };
        let producers = vec![
            producer(
                "iunci-backend",
                ContractKind::Rest,
                "POST /api/v1/referrals/track-click",
            ),
            producer("other", ContractKind::Rest, "GET /referrals"),
        ];
        let r = find_violations_routed("app", &consumed, &producers, &routing);
        assert_eq!(
            r.unverified,
            vec![
                "POST /referrals/track-click → iunci.store: no REST routes found there; likely served by iunci-backend (POST /api/v1/referrals/track-click)"
            ]
        );
    }

    #[test]
    fn test_self_hosts_and_domains() {
        assert!(looks_like_domain("iunci.app"));
        assert!(looks_like_domain("api.iunci.store"));
        assert!(!looks_like_domain("void-stack"));
        assert!(!looks_like_domain("config.toml"));
        assert_eq!(
            url_host("https://Api.Iunci.store:443/v1?x"),
            Some("api.iunci.store".into())
        );
        assert_eq!(url_host("/api/x"), None);
        assert!(is_self_url_key("NEXT_PUBLIC_SITE_URL"));
        assert!(!is_self_url_key("NEXT_PUBLIC_API_URL"));
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env.example"), "NEXT_PUBLIC_SITE_URL=\"https://shop.example.com\"\nNEXT_PUBLIC_API_URL=https://api.other.com\n").unwrap();
        std::fs::write(dir.path().join("CNAME"), "www.example.dev\n").unwrap();
        let project = Project {
            name: "iunci.store".into(),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        assert_eq!(
            project_self_hosts(&project),
            vec!["iunci.store", "shop.example.com", "www.example.dev"]
        );
    }

    #[test]
    fn test_unknown_producer_is_external_not_failure() {
        let consumed = vec![contract(
            ContractKind::Rest,
            ContractRole::Consumer,
            "POST /v1/charges",
        )];
        let producers = vec![producer("backend", ContractKind::Rest, "GET /api/users")];
        let report = find_violations("front", &consumed, &producers);
        assert!(report.violations.is_empty());
        assert_eq!(report.external, vec!["POST /v1/charges"]);
    }
}
