//! Cross-project GraphRAG: the primary project's graphrag plus a quick
//! semantic search of the same query in related indexed projects, linked by
//! the symbols they share.

use std::collections::HashSet;

use super::{CrossLink, CrossProjectRagResult, extract_symbols, graph_rag_search};
use crate::error::IndexError;
use crate::model::Project;
use crate::vector_index::search::SearchResult;

/// Minimum best-hit score for a related project to appear in an
/// unscoped cross search. Below this the hit is topical noise.
const RELATED_SCORE_FLOOR: f32 = 0.65;
/// Max related projects returned by an unscoped cross search.
const MAX_RELATED_PROJECTS: usize = 5;

/// Small dedicated pool for the cross-project fan-out: a few loads in
/// flight at once (I/O + graph parsing), without competing with the
/// indexing pool or the global rayon pool. Half the cores, 2–4 threads.
fn fanout_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        rayon::ThreadPoolBuilder::new()
            .num_threads((cpus / 2).clamp(2, 4))
            .thread_name(|i| format!("void-fanout-{}", i))
            .build()
            .expect("rayon pool with a fixed small thread count cannot fail to build")
    })
}

/// Cross-project GraphRAG: run a normal graphrag on `primary`, then for
/// every *other* project in `config` that has a semantic index, run a
/// quick `semantic_search` for the same query and look for symbols that
/// appear in both result sets. Any overlap is surfaced as a `CrossLink`.
///
/// Designed to be cheap: each related project gets a small top_k (3 by
/// default) and uses the existing semantic index — no extra index builds
/// happen here. Projects without an index are silently skipped, so this
/// works even when only some of a monorepo's projects are indexed.
pub fn graph_rag_search_cross(
    config: &crate::global_config::GlobalConfig,
    primary: &Project,
    query: &str,
    top_k: usize,
    depth: u8,
    related_projects: Option<&[String]>,
) -> Result<CrossProjectRagResult, IndexError> {
    let primary_result = graph_rag_search(primary, query, top_k, depth)?;

    // Collect symbols from the primary's combined chunks so we can
    // detect "this function name shows up over there too".
    let primary_symbols: HashSet<String> = primary_result
        .combined
        .iter()
        .flat_map(|c| extract_symbols(&c.chunk))
        .collect();

    let related_top_k = 3;
    let mut related: Vec<(String, Vec<SearchResult>)> = Vec::new();
    let mut cross_links: Vec<CrossLink> = Vec::new();

    // API contracts are the primary linking mechanism: a Flutter app calling
    // a generated gRPC stub and the Go backend implementing it share a
    // Service.Rpc key; a Next.js fetch('/api/...') matches a backend route.
    let primary_contracts = crate::vector_index::contracts::project_contracts(primary);

    // Candidates: other projects in scope that have a semantic index.
    let candidates: Vec<&Project> = config
        .projects
        .iter()
        .filter(|other| !other.name.eq_ignore_ascii_case(&primary.name))
        // Honor the explicit scope filter: with 27 indexed projects an
        // unscoped search floods the response with irrelevant hits.
        .filter(|other| {
            related_projects
                .is_none_or(|scope| scope.iter().any(|n| n.eq_ignore_ascii_case(&other.name)))
        })
        // Skip projects without an index — running search would fail
        // and "no index" isn't a meaningful link signal.
        .filter(|other| crate::vector_index::stats::index_exists(other))
        .collect();

    // Embed the query once for the whole fan-out (it used to be embedded
    // again for every related project), then search the related projects
    // in parallel on a small dedicated pool. Each search loads its index
    // transiently (memory-mapped, dropped right after), so this neither
    // pins every project's graph in memory nor walks ~50 loads one by one.
    let query_emb = crate::vector_index::search::embed_query(query)?;
    let searched: Vec<(&Project, Vec<SearchResult>)> = {
        use rayon::prelude::*;
        fanout_pool().install(|| {
            candidates
                .par_iter()
                .filter_map(|other| {
                    match crate::vector_index::search::semantic_search_transient(
                        other,
                        query,
                        &query_emb,
                        related_top_k,
                    ) {
                        Ok(h) if !h.is_empty() => Some((*other, h)),
                        _ => None,
                    }
                })
                .collect()
        })
    };

    for (other, hits) in searched {
        // Contract matching first (gRPC service/rpc pairs, REST routes,
        // vendored proto copies) — these are real API dependencies.
        let other_contracts = crate::vector_index::contracts::project_contracts(other);
        let clinks =
            crate::vector_index::contracts::contract_links(&primary_contracts, &other_contracts);
        let had_contract_link = !clinks.is_empty();
        for cl in clinks {
            cross_links.push(CrossLink {
                from_project: primary.name.clone(),
                to_project: other.name.clone(),
                via: cl.via,
                shared_symbols: cl.keys,
            });
        }

        // Shared symbols remain only as a last-resort fallback, explicitly
        // labeled weak — same names ≠ same API.
        if !had_contract_link {
            let other_symbols: HashSet<String> = hits
                .iter()
                .flat_map(|r| extract_symbols(&r.chunk))
                .collect();

            let mut shared: Vec<String> = primary_symbols
                .intersection(&other_symbols)
                .cloned()
                .collect();
            shared.sort();

            if !shared.is_empty() {
                cross_links.push(CrossLink {
                    from_project: primary.name.clone(),
                    to_project: other.name.clone(),
                    via: "shared symbols (weak)".to_string(),
                    shared_symbols: shared,
                });
            }
        }

        related.push((other.name.clone(), hits));
    }

    // Unscoped searches get a relevance floor + top-N cap; an explicit
    // scope means the user asked for those projects — return them all.
    let (related, related_omitted) = if related_projects.is_some() {
        (related, 0)
    } else {
        filter_related(related, RELATED_SCORE_FLOOR, MAX_RELATED_PROJECTS)
    };

    Ok(CrossProjectRagResult {
        primary: primary_result,
        related,
        cross_links,
        related_omitted,
    })
}

/// Drop related-project hits below `floor`, then keep the top `cap`
/// projects by best-hit score. Returns the kept list plus how many
/// projects were omitted (floored out or beyond the cap).
pub(super) fn filter_related(
    related: Vec<(String, Vec<SearchResult>)>,
    floor: f32,
    cap: usize,
) -> (Vec<(String, Vec<SearchResult>)>, usize) {
    let total = related.len();
    let mut kept: Vec<(String, Vec<SearchResult>)> = related
        .into_iter()
        .filter_map(|(name, hits)| {
            let strong: Vec<SearchResult> = hits.into_iter().filter(|h| h.score >= floor).collect();
            if strong.is_empty() {
                None
            } else {
                Some((name, strong))
            }
        })
        .collect();
    kept.sort_by(|a, b| {
        let best = |hits: &[SearchResult]| hits.iter().map(|h| h.score).fold(f32::MIN, f32::max);
        best(&b.1)
            .partial_cmp(&best(&a.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    kept.truncate(cap);
    let omitted = total - kept.len();
    (kept, omitted)
}
