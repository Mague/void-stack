//! `void contracts check`: fail when a consumed contract drifted.
//!
//! Verification mode over the existing gRPC/REST contract detection: for
//! every contract a project CONSUMES, look for a producer among the
//! registered projects. A consumed RPC whose service exists but no longer
//! exposes that method, or a REST route whose path exists under a
//! different method/signature, is a VIOLATION (non-zero exit in the CLI).
//! Consumed contracts with no related producer anywhere are reported as
//! external (Stripe, GitHub...) and never fail the check.

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
}

/// Check one project's consumed contracts against every registered
/// producer. Scans all projects' contracts (cached per file SHA).
pub fn check_contracts(config: &GlobalConfig, project: &Project) -> ContractCheckReport {
    let consumed: Vec<ApiContract> = project_contracts(project)
        .into_iter()
        .filter(|c| c.role == ContractRole::Consumer)
        .collect();

    let mut producers: Vec<(String, ApiContract)> = Vec::new();
    for p in &config.projects {
        if !std::path::Path::new(&crate::runner::local::strip_win_prefix(&p.path)).exists() {
            continue;
        }
        for c in project_contracts(p) {
            if c.role == ContractRole::Producer {
                producers.push((p.name.clone(), c));
            }
        }
    }

    find_violations(&project.name, &consumed, &producers)
}

/// Pure matching core (unit-testable without a filesystem).
pub fn find_violations(
    consumer_project: &str,
    consumed: &[ApiContract],
    producers: &[(String, ApiContract)],
) -> ContractCheckReport {
    let mut matched = 0usize;
    let mut external = Vec::new();
    let mut violations = Vec::new();

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
            matched += 1;
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
                } else {
                    external.push(c.key.clone());
                }
            }
            ContractKind::GrpcProtoHash => unreachable!("filtered above"),
        }
    }

    external.sort();
    external.dedup();
    ContractCheckReport {
        project: consumer_project.to_string(),
        consumed: consumed
            .iter()
            .filter(|c| c.kind != ContractKind::GrpcProtoHash)
            .count(),
        matched,
        external,
        violations,
    }
}

/// `METHOD /path` keys match when methods are equal and paths are
/// param-compatible.
fn rest_keys_match(a: &str, b: &str) -> bool {
    match (a.split_once(' '), b.split_once(' ')) {
        (Some((ma, pa)), Some((mb, pb))) => ma.eq_ignore_ascii_case(mb) && rest_paths_match(pa, pb),
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
