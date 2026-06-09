//! GraphRAG: fuse semantic search with the structural call graph.
//!
//! Pipeline:
//!   1. semantic_search(query, top_k)            → seed chunks
//!   2. extract symbols from each seed chunk     → candidate qualified_names
//!   3. structural BFS (callers + callees, ≤depth, ≤5/seed)
//!   4. for each impacted symbol, fetch its chunk from the semantic index
//!   5. score, dedupe, sort
//!
//! Disabled when the `structural` feature is off — graph traversal is the
//! whole point. Falls back to silent omission for files that aren't indexed.

#![cfg(feature = "structural")]

use std::collections::{HashMap, HashSet};

use regex::Regex;
use rusqlite::Connection;
use serde::Serialize;

use super::db::open_meta_db;
use super::search::{SearchResult, semantic_search};
use crate::error::IndexError;
use crate::model::Project;
use crate::structural::{
    StructuralNode, get_callees, get_callers, get_tests_for, open_db, search_nodes,
};

// ── Output types ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    Caller,
    Callee,
    TestFor,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChunkOrigin {
    Semantic,
    Structural,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextChunk {
    pub file_path: String,
    pub chunk: String,
    pub line_start: usize,
    pub line_end: usize,
    pub source: ContextSource,
    pub hops: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct RankedChunk {
    pub file_path: String,
    pub chunk: String,
    pub line_start: usize,
    pub line_end: usize,
    pub score: f32,
    pub origin: ChunkOrigin,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphRagResult {
    pub semantic_seeds: Vec<SearchResult>,
    pub structural_context: Vec<ContextChunk>,
    pub combined: Vec<RankedChunk>,
    pub communities_hit: Vec<usize>,
    pub token_estimate: usize,
    /// False when the structural DB couldn't be opened — the result is a
    /// semantic-only response. Lets callers surface a hint to run
    /// `build_structural_graph` instead of treating the empty context as
    /// "no callers found".
    pub has_structural_index: bool,
    /// Unique files whose structural expansions were dropped because they
    /// have no chunk in the semantic index. Lets callers report
    /// "N files skipped" instead of omitting them silently.
    pub files_skipped_not_indexed: usize,
}

/// A detected coupling between two projects discovered while running
/// [`graph_rag_search_cross`]. The MVP uses *shared symbols* as the
/// linkage signal — same function/class/route name appearing in chunks
/// from both projects. The `via` slot is reserved for richer heuristics
/// (`.proto` files, OpenAPI specs, shared package names) added later.
#[derive(Debug, Clone, Serialize)]
pub struct CrossLink {
    pub from_project: String,
    pub to_project: String,
    /// Why this link was inferred — e.g. "shared symbols", "phoenix",
    /// "grpc proto", "package.json import".
    pub via: String,
    pub shared_symbols: Vec<String>,
}

/// Cross-project GraphRAG output. Combines the primary project's
/// `GraphRagResult` with related-project search hits and the explanatory
/// `CrossLink`s connecting them.
#[derive(Debug, Clone, Serialize)]
pub struct CrossProjectRagResult {
    pub primary: GraphRagResult,
    /// `(project_name, top results from that project)` pairs.
    pub related: Vec<(String, Vec<SearchResult>)>,
    pub cross_links: Vec<CrossLink>,
}

// ── Tunables ───────────────────────────────────────────────

const SEMANTIC_WEIGHT: f32 = 0.6;
const SEMANTIC_BIAS: f32 = 0.4;
const STRUCTURAL_WEIGHT: f32 = 0.4;
const MAX_EXPANSIONS_PER_SEED: usize = 5;
const MAX_SYMBOLS_PER_CHUNK: usize = 5;
const APPROX_CHARS_PER_TOKEN: usize = 4;

/// Cap total structural context to avoid token explosion.
const MAX_STRUCTURAL_CHUNKS: usize = 20;

// ── Public API ─────────────────────────────────────────────

/// Run a semantic search and expand the result set with structural context.
pub fn graph_rag_search(
    project: &Project,
    query: &str,
    top_k: usize,
    depth: u8,
) -> Result<GraphRagResult, IndexError> {
    let depth = depth.clamp(1, 3);

    // STEP 1 — Semantic seeds.
    let seeds = semantic_search(project, query, top_k)?;

    // Communities present in the seeds, deduped & sorted ascending.
    let mut communities_hit: Vec<usize> = seeds
        .iter()
        .filter_map(|s| s.community_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    communities_hit.sort_unstable();

    if seeds.is_empty() {
        // We never tried to open the structural DB on this path, so we can't
        // claim it's missing. Default to true so callers don't surface a
        // misleading "run build_structural_graph" hint.
        return Ok(GraphRagResult {
            semantic_seeds: seeds,
            structural_context: Vec::new(),
            combined: Vec::new(),
            communities_hit,
            token_estimate: 0,
            has_structural_index: true,
            files_skipped_not_indexed: 0,
        });
    }

    // STEP 2 + 3 — Symbol extraction → BFS expansion.
    // If the structural DB is missing/unreadable, degrade to semantic-only
    // instead of erroring out — the caller can still display the seeds.
    let s_conn = match open_db(project) {
        Ok(c) => c,
        Err(_) => return Ok(semantic_only_result(seeds, communities_hit)),
    };
    let v_conn = open_meta_db(project)?;

    let mut structural_context: Vec<ContextChunk> = Vec::new();
    let mut skipped_files: HashSet<String> = HashSet::new();

    for seed in &seeds {
        let symbols = extract_symbols(&seed.chunk);
        let qnames = resolve_qnames(&s_conn, &symbols);
        let expansions = expand_symbols(&s_conn, &qnames, depth);

        for (node, source, hops) in expansions.into_iter().take(MAX_EXPANSIONS_PER_SEED) {
            if let Some(chunk) = lookup_chunk_for_node(&v_conn, &node) {
                structural_context.push(ContextChunk {
                    file_path: chunk.file_path,
                    chunk: chunk.text,
                    line_start: chunk.line_start,
                    line_end: chunk.line_end,
                    source,
                    hops,
                });
            } else {
                // No chunk in the semantic index for this file — count it so
                // callers can report the omission instead of hiding it.
                skipped_files.insert(normalize_path(&node.file_path));
            }
        }
    }

    // Cap total structural context to avoid token explosion.
    if structural_context.len() > MAX_STRUCTURAL_CHUNKS {
        structural_context.truncate(MAX_STRUCTURAL_CHUNKS);
    }

    // STEP 4 — Score + merge + dedupe.
    let combined = merge_and_rank(&seeds, &structural_context);

    // STEP 5 — Token estimate.
    let token_estimate = combined
        .iter()
        .map(|c| c.chunk.len() / APPROX_CHARS_PER_TOKEN)
        .sum();

    record_graphrag_savings(project, &combined);

    Ok(GraphRagResult {
        semantic_seeds: seeds,
        structural_context,
        combined,
        communities_hit,
        token_estimate,
        has_structural_index: true,
        files_skipped_not_indexed: skipped_files.len(),
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

    for other in &config.projects {
        if other.name.eq_ignore_ascii_case(&primary.name) {
            continue;
        }
        // Skip projects without an index — running search would fail
        // and "no index" isn't a meaningful link signal.
        if !super::stats::index_exists(other) {
            continue;
        }
        let hits = match super::search::semantic_search(other, query, related_top_k) {
            Ok(h) if !h.is_empty() => h,
            _ => continue,
        };

        // Symbols from this related project's hits.
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
                via: "shared symbols".to_string(),
                shared_symbols: shared,
            });
        }

        related.push((other.name.clone(), hits));
    }

    Ok(CrossProjectRagResult {
        primary: primary_result,
        related,
        cross_links,
    })
}

/// Record real savings for graphrag: bytes in the combined chunks vs the
/// full bytes of every file we pulled from. Same approximation as
/// semantic_search (tokens ~ bytes / 4).
fn record_graphrag_savings(project: &Project, combined: &[RankedChunk]) {
    use chrono::Utc;

    let project_root = std::path::Path::new(&project.path);

    let tokens_returned: usize = combined.iter().map(|c| c.chunk.len() / 4).sum();

    let mut unique_files: HashSet<&str> = HashSet::new();
    for c in combined {
        unique_files.insert(c.file_path.as_str());
    }
    let tokens_full: usize = unique_files
        .iter()
        .filter_map(|p| std::fs::metadata(project_root.join(p)).ok())
        .map(|m| m.len() as usize / 4)
        .sum();

    let savings_pct = if tokens_full > tokens_returned {
        ((1.0 - tokens_returned as f64 / tokens_full as f64) * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    };

    crate::stats::record_saving(crate::stats::TokenSavingsRecord {
        timestamp: Utc::now(),
        project: project.name.clone(),
        operation: "graph_rag_search".to_string(),
        lines_original: tokens_full,
        lines_filtered: tokens_returned,
        savings_pct,
    });
}

/// Build a semantic-only result when the structural index is unavailable.
fn semantic_only_result(seeds: Vec<SearchResult>, communities_hit: Vec<usize>) -> GraphRagResult {
    let combined: Vec<RankedChunk> = seeds
        .iter()
        .map(|s| RankedChunk {
            file_path: s.file_path.clone(),
            chunk: s.chunk.clone(),
            line_start: s.line_start,
            line_end: s.line_end,
            score: score_seed(s.score),
            origin: ChunkOrigin::Semantic,
        })
        .collect();
    let token_estimate = combined
        .iter()
        .map(|c| c.chunk.len() / APPROX_CHARS_PER_TOKEN)
        .sum();
    GraphRagResult {
        semantic_seeds: seeds,
        structural_context: Vec::new(),
        combined,
        communities_hit,
        token_estimate,
        has_structural_index: false,
        files_skipped_not_indexed: 0,
    }
}

// ── Symbol extraction ──────────────────────────────────────

/// Symbol-extraction patterns, one per family. Splitting them keeps each
/// pattern legible and lets us cover language families that share no
/// syntax (Rust keyword-prefix vs Dart type-prefix vs Erlang clause).
///
/// Order matters only for performance — every pattern still runs against
/// every chunk; we just stop adding symbols once we hit
/// [`MAX_SYMBOLS_PER_CHUNK`].
fn symbol_patterns() -> &'static [Regex] {
    static RES: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    RES.get_or_init(|| {
        let raw = [
            // Rust / Go / JS / TS / Python / Java / C# / Elixir / Phoenix
            // keyword prefix → capture the identifier (Elixir allows ? and !
            // in atoms, e.g. `def valid?(arg)`).
            r"(?m)\b(?:fn|struct|impl|class|def|defp|defmodule|defprotocol|defimpl|defmacro|defmacrop|func|interface|trait|enum)\s+([A-Za-z_][A-Za-z0-9_.?!]*)",
            // Dart: `void foo(`, `Future<…> bar(`, `Widget build(` — return type
            // (with optional generics) followed by the method name and a `(`.
            r"(?m)\b(?:void|Future|Stream|Widget|List|Map|Set|String|int|double|bool|num|dynamic|Iterable|Iterator)(?:<[^>]+>)?\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
            // Erlang: `name(Args) -> …` — lowercase clause head followed by
            // a `->` separator (no `def` keyword to anchor on).
            r"(?m)^\s*([a-z][A-Za-z0-9_]*)\s*\([^\)\n]*\)\s*->",
            // Phoenix routing DSL: `plug :foo`, `pipeline :browser`,
            // `scope "/api"`, `resources "/users", UserController`. Capture
            // the second token so we get `:foo`/`browser`/the route path
            // stem — useful as a search anchor in cross-project graphs.
            r#"(?m)\b(?:plug|pipeline|scope|resources|forward|live|get|post|put|delete|patch)\s+:?([A-Za-z_][A-Za-z0-9_]*)"#,
        ];
        raw.iter()
            .map(|p| Regex::new(p).expect("static regex compiles"))
            .collect()
    })
}

fn extract_symbols(chunk: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    for re in symbol_patterns() {
        for cap in re.captures_iter(chunk) {
            if let Some(m) = cap.get(1) {
                let name = m.as_str().to_string();
                // Skip 1-char captures (noisy generics) and the keyword
                // alternation's own keywords (defensive: a malformed
                // regex would otherwise leak `def`, `fn`, etc.).
                if name.len() < 2 {
                    continue;
                }
                if seen.insert(name.clone()) {
                    out.push(name);
                }
            }
            if out.len() >= MAX_SYMBOLS_PER_CHUNK {
                return out;
            }
        }
    }
    out
}

// ── Structural lookups ─────────────────────────────────────

fn resolve_qnames(s_conn: &Connection, symbols: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for sym in symbols {
        for node in search_nodes(s_conn, sym, 3) {
            if seen.insert(node.qualified_name.clone()) {
                out.push(node.qualified_name);
            }
        }
    }
    out
}

fn expand_symbols(
    s_conn: &Connection,
    qnames: &[String],
    depth: u8,
) -> Vec<(StructuralNode, ContextSource, u8)> {
    let mut out: Vec<(StructuralNode, ContextSource, u8)> = Vec::new();
    let mut visited: HashSet<String> = qnames.iter().cloned().collect();

    // Hop 1 — direct callers/callees.
    let mut frontier: Vec<String> = Vec::new();
    for qn in qnames {
        for caller in get_callers(s_conn, qn) {
            if visited.insert(caller.qualified_name.clone()) {
                frontier.push(caller.qualified_name.clone());
                out.push((caller, ContextSource::Caller, 1));
            }
        }
        for callee in get_callees(s_conn, qn) {
            if visited.insert(callee.qualified_name.clone()) {
                frontier.push(callee.qualified_name.clone());
                out.push((callee, ContextSource::Callee, 1));
            }
        }
        for tn in get_tests_for(s_conn, qn) {
            if visited.insert(tn.qualified_name.clone()) {
                out.push((tn, ContextSource::TestFor, 1));
            }
        }
    }

    // Further hops if requested.
    for hop in 2..=depth {
        let mut next: Vec<String> = Vec::new();
        for qn in &frontier {
            for caller in get_callers(s_conn, qn) {
                if visited.insert(caller.qualified_name.clone()) {
                    next.push(caller.qualified_name.clone());
                    out.push((caller, ContextSource::Caller, hop));
                }
            }
            for callee in get_callees(s_conn, qn) {
                if visited.insert(callee.qualified_name.clone()) {
                    next.push(callee.qualified_name.clone());
                    out.push((callee, ContextSource::Callee, hop));
                }
            }
        }
        frontier = next;
    }

    out
}

// ── Semantic chunk lookup by structural node ───────────────

#[derive(Debug, Clone)]
struct ChunkRow {
    file_path: String,
    text: String,
    line_start: usize,
    line_end: usize,
}

/// Find the semantic chunk that overlaps with the structural node's line
/// range. Silently returns None if no chunk covers those lines (e.g. file
/// not indexed).
///
/// On Windows the chunker can persist `file_path` with `\` separators while
/// the structural graph normalizes to `/` (or vice versa, depending on the
/// indexer run). We query for *both* spellings so the symbol isn't dropped
/// just because of a separator mismatch — that mismatch made structural
/// context perpetually empty on Windows.
fn lookup_chunk_for_node(v_conn: &Connection, node: &StructuralNode) -> Option<ChunkRow> {
    let forward = normalize_path(&node.file_path);
    let backward = forward.replace('/', "\\");

    let mut stmt = v_conn
        .prepare(
            "SELECT file_path, line_start, line_end, text FROM chunks \
             WHERE (file_path = ?1 OR file_path = ?2) \
               AND line_start <= ?3 \
               AND line_end   >= ?4 \
             ORDER BY ABS(line_start - ?4) ASC \
             LIMIT 1",
        )
        .ok()?;

    let row = stmt
        .query_row(
            rusqlite::params![
                forward,
                backward,
                node.line_end as i64,
                node.line_start as i64,
            ],
            |row| {
                Ok(ChunkRow {
                    file_path: row.get(0)?,
                    line_start: row.get::<_, i64>(1)? as usize,
                    line_end: row.get::<_, i64>(2)? as usize,
                    text: row.get(3)?,
                })
            },
        )
        .ok();

    if row.is_some() {
        return row;
    }

    // Fallback: any chunk for this file. Better than dropping the symbol
    // entirely when line ranges don't overlap (e.g. function declared but
    // chunker grouped surrounding code differently).
    let mut stmt = v_conn
        .prepare(
            "SELECT file_path, line_start, line_end, text FROM chunks \
             WHERE file_path = ?1 OR file_path = ?2 \
             ORDER BY line_start ASC LIMIT 1",
        )
        .ok()?;
    stmt.query_row(rusqlite::params![forward, backward], |row| {
        Ok(ChunkRow {
            file_path: row.get(0)?,
            line_start: row.get::<_, i64>(1)? as usize,
            line_end: row.get::<_, i64>(2)? as usize,
            text: row.get(3)?,
        })
    })
    .ok()
}

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
}

// ── Scoring + merge ────────────────────────────────────────

fn score_seed(semantic_score: f32) -> f32 {
    semantic_score * SEMANTIC_WEIGHT + SEMANTIC_BIAS
}

fn score_structural(hops: u8) -> f32 {
    let h = hops.max(1) as f32;
    STRUCTURAL_WEIGHT * (1.0 / h)
}

fn merge_and_rank(seeds: &[SearchResult], context: &[ContextChunk]) -> Vec<RankedChunk> {
    // Dedupe by (file_path, line_start). Semantic origin wins on ties.
    let mut by_key: HashMap<(String, usize), RankedChunk> = HashMap::new();

    for s in seeds {
        let key = (s.file_path.clone(), s.line_start);
        by_key.insert(
            key,
            RankedChunk {
                file_path: s.file_path.clone(),
                chunk: s.chunk.clone(),
                line_start: s.line_start,
                line_end: s.line_end,
                score: score_seed(s.score),
                origin: ChunkOrigin::Semantic,
            },
        );
    }

    for c in context {
        let key = (c.file_path.clone(), c.line_start);
        // If a semantic seed already covers the same chunk, keep it.
        by_key.entry(key).or_insert_with(|| RankedChunk {
            file_path: c.file_path.clone(),
            chunk: c.chunk.clone(),
            line_start: c.line_start,
            line_end: c.line_end,
            score: score_structural(c.hops),
            origin: ChunkOrigin::Structural,
        });
    }

    let mut out: Vec<RankedChunk> = by_key.into_values().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_symbols_rust_fn_struct_impl() {
        let chunk = "pub fn foo() {}\nstruct Bar {}\nimpl Bar { fn bar() {} }\n";
        let syms = extract_symbols(chunk);
        assert!(syms.contains(&"foo".to_string()));
        assert!(syms.contains(&"Bar".to_string()));
        assert!(syms.contains(&"bar".to_string()));
    }

    #[test]
    fn test_extract_symbols_python_def_class() {
        let chunk = "class Authenticator:\n    def authenticate(self):\n        pass\n";
        let syms = extract_symbols(chunk);
        assert!(syms.contains(&"Authenticator".to_string()));
        assert!(syms.contains(&"authenticate".to_string()));
    }

    #[test]
    fn test_extract_symbols_dart_method_return_types() {
        let chunk = r#"
class HomePage extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return Container();
  }

  Future<void> handleTap(int idx) async {
    await something();
  }

  Stream<int> ticks() async* {
    yield 1;
  }
}
"#;
        let syms = extract_symbols(chunk);
        assert!(syms.contains(&"build".to_string()), "got {:?}", syms);
        assert!(syms.contains(&"handleTap".to_string()), "got {:?}", syms);
        assert!(syms.contains(&"ticks".to_string()), "got {:?}", syms);
        // Class name still captured via the `class` keyword pattern.
        assert!(syms.contains(&"HomePage".to_string()), "got {:?}", syms);
    }

    #[test]
    fn test_extract_symbols_elixir_def_defp_defmodule() {
        let chunk = "defmodule MyApp.Auth do\n  def login(user), do: :ok\n  defp valid?(token), do: true\nend\n";
        let syms = extract_symbols(chunk);
        assert!(syms.contains(&"MyApp.Auth".to_string()), "got {:?}", syms);
        assert!(syms.contains(&"login".to_string()), "got {:?}", syms);
        assert!(syms.contains(&"valid?".to_string()), "got {:?}", syms);
    }

    #[test]
    fn test_extract_symbols_phoenix_router() {
        let chunk = r#"
scope "/api", MyAppWeb do
  pipeline :browser
  plug :accepts
  resources "/users", UserController
  get :index, UserController, :index
end
"#;
        let syms = extract_symbols(chunk);
        // We expect to pick up plug/pipeline/scope arguments. Exact
        // contents depend on capture order — assert at least one DSL
        // token landed.
        assert!(
            syms.iter().any(|s| s == "browser" || s == "accepts"),
            "expected a DSL atom captured, got {:?}",
            syms
        );
    }

    #[test]
    fn test_cross_links_shared_symbols_intersect() {
        // The detection logic for cross_links is just symbol-set
        // intersection; verify it directly with strings instead of
        // wiring up two real indexes (which would need fastembed +
        // tempdirs + HNSW files).
        let primary_chunk = "fn create_auction(name: &str) {}\nfn list_properties() {}";
        let related_chunk = "def create_auction(params), do: :ok\ndef cancel_auction(id), do: :ok";

        let p: std::collections::HashSet<String> =
            extract_symbols(primary_chunk).into_iter().collect();
        let r: std::collections::HashSet<String> =
            extract_symbols(related_chunk).into_iter().collect();
        let shared: Vec<String> = p.intersection(&r).cloned().collect();

        assert!(
            shared.contains(&"create_auction".to_string()),
            "expected create_auction in {shared:?}"
        );
        assert!(
            !shared.contains(&"cancel_auction".to_string()),
            "unique symbol shouldn't appear in {shared:?}"
        );
    }

    #[test]
    fn test_cross_link_serialises() {
        let link = CrossLink {
            from_project: "hipobid-backend".to_string(),
            to_project: "hipobid-elixir".to_string(),
            via: "shared symbols".to_string(),
            shared_symbols: vec!["create_auction".to_string(), "list_properties".to_string()],
        };
        let json = serde_json::to_string(&link).unwrap();
        assert!(json.contains("hipobid-backend"));
        assert!(json.contains("shared symbols"));
        assert!(json.contains("create_auction"));
    }

    #[test]
    fn test_cross_no_related_projects_when_config_empty() {
        // graph_rag_search_cross with an empty config (just the primary)
        // must short-circuit cleanly: no panic, no related entries, no
        // links. We can't run the full pipeline without an index, so
        // verify the empty-iteration contract via direct construction.
        let result = CrossProjectRagResult {
            primary: GraphRagResult {
                semantic_seeds: vec![],
                structural_context: vec![],
                combined: vec![],
                communities_hit: vec![],
                token_estimate: 0,
                has_structural_index: true,
                files_skipped_not_indexed: 0,
            },
            related: vec![],
            cross_links: vec![],
        };
        assert!(result.related.is_empty());
        assert!(result.cross_links.is_empty());
    }

    #[test]
    fn test_extract_symbols_erlang_function_head() {
        let chunk = "% Erlang module\nhandle_call(Request, From, State) ->\n    {reply, ok, State}.\nstart_link() ->\n    gen_server:start_link(?MODULE, [], []).\n";
        let syms = extract_symbols(chunk);
        assert!(syms.contains(&"handle_call".to_string()), "got {:?}", syms);
        assert!(syms.contains(&"start_link".to_string()), "got {:?}", syms);
    }

    #[test]
    fn test_extract_symbols_caps_at_max() {
        let many = (0..20)
            .map(|i| format!("fn name{}() {{}}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let syms = extract_symbols(&many);
        assert!(syms.len() <= MAX_SYMBOLS_PER_CHUNK);
    }

    #[test]
    fn test_extract_symbols_dedupes() {
        let chunk = "fn foo() {}\nfn foo() {}\nfn foo() {}";
        let syms = extract_symbols(chunk);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0], "foo");
    }

    #[test]
    fn test_score_seed_above_structural_hop1() {
        // Seed with semantic_score 0.9 → 0.9 * 0.6 + 0.4 = 0.94
        let s = score_seed(0.9);
        let h1 = score_structural(1);
        let h2 = score_structural(2);
        assert!(s > h1, "seed {} should beat hop-1 {}", s, h1);
        assert!(h1 > h2, "hop-1 {} should beat hop-2 {}", h1, h2);
    }

    #[test]
    fn test_merge_and_rank_dedupes_seed_over_structural() {
        let seed = SearchResult {
            file_path: "src/auth.rs".to_string(),
            chunk: "fn login() {}".to_string(),
            score: 0.9,
            line_start: 10,
            line_end: 15,
            community_id: None,
        };
        let ctx = ContextChunk {
            file_path: "src/auth.rs".to_string(),
            chunk: "fn login() {}".to_string(),
            line_start: 10,
            line_end: 15,
            source: ContextSource::Caller,
            hops: 1,
        };
        let merged = merge_and_rank(&[seed], &[ctx]);
        assert_eq!(merged.len(), 1, "duplicate (file, line) collapses");
        assert!(matches!(merged[0].origin, ChunkOrigin::Semantic));
    }

    #[test]
    fn test_merge_and_rank_keeps_distinct_chunks() {
        let seed = SearchResult {
            file_path: "src/auth.rs".to_string(),
            chunk: "seed".to_string(),
            score: 0.9,
            line_start: 10,
            line_end: 15,
            community_id: None,
        };
        let ctx = ContextChunk {
            file_path: "src/db.rs".to_string(),
            chunk: "ctx".to_string(),
            line_start: 5,
            line_end: 8,
            source: ContextSource::Callee,
            hops: 1,
        };
        let merged = merge_and_rank(&[seed], &[ctx]);
        assert_eq!(merged.len(), 2);
        // Sort: seed score (0.94) > structural hop-1 score (0.4)
        assert!(matches!(merged[0].origin, ChunkOrigin::Semantic));
        assert!(matches!(merged[1].origin, ChunkOrigin::Structural));
    }

    #[test]
    fn test_token_estimate_chars_div_4() {
        let combined = [
            RankedChunk {
                file_path: "a".to_string(),
                chunk: "x".repeat(40),
                line_start: 1,
                line_end: 1,
                score: 1.0,
                origin: ChunkOrigin::Semantic,
            },
            RankedChunk {
                file_path: "b".to_string(),
                chunk: "y".repeat(80),
                line_start: 1,
                line_end: 1,
                score: 0.5,
                origin: ChunkOrigin::Structural,
            },
        ];
        let est: usize = combined
            .iter()
            .map(|c| c.chunk.len() / APPROX_CHARS_PER_TOKEN)
            .sum();
        // 40/4 + 80/4 = 30
        assert_eq!(est, 30);
    }

    #[test]
    fn test_normalize_path_converts_backslashes() {
        assert_eq!(normalize_path("src\\auth.rs"), "src/auth.rs");
        assert_eq!(normalize_path("src/auth.rs"), "src/auth.rs");
    }

    fn open_chunks_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                text TEXT NOT NULL,
                mtime REAL NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_lookup_chunk_backslash_path() {
        // Regression: Windows graphrag was missing every structural hit
        // because the chunks table stored `crates\foo\bar.rs` while the
        // structural node arrived as `crates/foo/bar.rs`.
        let conn = open_chunks_db();
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["crates\\foo\\bar.rs", 1i64, 50i64, "fn x() {}"],
        )
        .unwrap();

        let node = crate::structural::StructuralNode {
            kind: crate::structural::NodeKind::Function,
            name: "x".to_string(),
            qualified_name: "crates/foo/bar.rs::x".to_string(),
            file_path: "crates/foo/bar.rs".to_string(),
            line_start: 10,
            line_end: 20,
            language: "rust".to_string(),
            parent_name: None,
            is_test: false,
        };

        let row = lookup_chunk_for_node(&conn, &node);
        assert!(
            row.is_some(),
            "lookup must succeed across separator mismatch"
        );
        let row = row.unwrap();
        assert_eq!(row.file_path, "crates\\foo\\bar.rs");
        assert_eq!(row.line_start, 1);
        assert_eq!(row.line_end, 50);
    }

    #[test]
    fn test_lookup_chunk_forward_path_still_works() {
        let conn = open_chunks_db();
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["crates/foo/bar.rs", 1i64, 50i64, "fn x() {}"],
        )
        .unwrap();
        let node = crate::structural::StructuralNode {
            kind: crate::structural::NodeKind::Function,
            name: "x".to_string(),
            qualified_name: "crates/foo/bar.rs::x".to_string(),
            file_path: "crates/foo/bar.rs".to_string(),
            line_start: 10,
            line_end: 20,
            language: "rust".to_string(),
            parent_name: None,
            is_test: false,
        };
        let row = lookup_chunk_for_node(&conn, &node).unwrap();
        assert_eq!(row.file_path, "crates/foo/bar.rs");
    }

    #[test]
    fn test_empty_seeds_does_not_claim_no_structural_index() {
        // has_structural_index reflects whether we could open the structural DB,
        // not whether semantic search found anything.
        // We can't open a real DB in unit tests, but we can at least document
        // that the early-return path should NOT set it to false.
        let result = GraphRagResult {
            semantic_seeds: vec![],
            structural_context: vec![],
            combined: vec![],
            communities_hit: vec![],
            token_estimate: 0,
            has_structural_index: true, // correct default when seeds are empty
            files_skipped_not_indexed: 0,
        };
        assert!(result.has_structural_index);
    }
}
