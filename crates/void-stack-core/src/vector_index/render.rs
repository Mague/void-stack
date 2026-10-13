//! How search hits are written back to an agent.
//!
//! A full chunk is 40–150 lines; most of the time the agent only needs to
//! know *where* the answer is and see the lines that matched. Compact mode
//! prints the location, the symbol that opens the chunk and a short
//! preview chosen by the query terms; the agent expands what it needs with
//! `get_chunk`.

use super::search::{ChunkKind, SearchResult, chunk_kind};

/// How much of each hit to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// Location, symbol and a few matching lines (default).
    Compact,
    /// The whole chunk.
    Full,
    /// Full for a confident top hit (score ≥ [`AUTO_FULL_SCORE`]), compact
    /// for the rest.
    Auto,
}

impl Detail {
    pub fn parse(s: Option<&str>) -> Detail {
        match s.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("full") => Detail::Full,
            Some("auto") => Detail::Auto,
            _ => Detail::Compact,
        }
    }
}

pub const AUTO_FULL_SCORE: f32 = 0.75;
/// Preview lines per hit in compact mode.
pub const PREVIEW_LINES: usize = 5;
const MAX_LINE_CHARS: usize = 140;

/// Lines the indexer prepends to a chunk for embedding context (`// Used
/// by:`, `// Imports:`, `// <file path>`, `// (continued) <signature>`);
/// they are not in the source file, so previews skip them and line
/// numbers ignore them.
fn context_header_len(text: &str, file_path: &str) -> usize {
    let path_line = format!("// {}", file_path);
    text.lines()
        .take(4)
        .take_while(|l| {
            l.starts_with("// Used by: ")
                || l.starts_with("// Imports: ")
                || l.starts_with("// (continued) ")
                || *l == path_line
        })
        .count()
}

/// The signature of a function split across chunks, from the indexer's
/// `// (continued) <signature>` header.
fn continued_signature(text: &str) -> Option<&str> {
    text.lines()
        .take(4)
        .find_map(|l| l.strip_prefix("// (continued) "))
}

/// The chunk's source lines (context header removed) with their real line
/// numbers.
fn source_lines(r: &SearchResult) -> Vec<(usize, &str)> {
    let skip = context_header_len(&r.chunk, &r.file_path);
    r.chunk
        .lines()
        .skip(skip)
        .enumerate()
        .map(|(i, l)| (r.line_start + i, l))
        .collect()
}

const DEF_PREFIXES: &[&str] = &[
    "pub fn ",
    "fn ",
    "pub async fn ",
    "async fn ",
    "pub(crate) fn ",
    "pub(crate) async fn ",
    "pub(super) fn ",
    "impl ",
    "impl<",
    "pub struct ",
    "struct ",
    "pub enum ",
    "enum ",
    "pub trait ",
    "trait ",
    "pub type ",
    "def ",
    "async def ",
    "class ",
    "export class ",
    "export default class ",
    "abstract class ",
    "func ",
    "function ",
    "export function ",
    "export async function ",
    "export default function ",
    "async function ",
    "interface ",
    "export interface ",
    "type ",
    "export type ",
    "export const ",
    "const ",
    "mixin ",
    "extension ",
    "public ",
    "private ",
    "protected ",
    "static ",
    "defmodule ",
    "defp ",
    "message ",
    "service ",
    "rpc ",
    "int ",
    "void ",
    "double ",
    "bool ",
    "string ",
];

/// The definition that opens the chunk, if any — `fn search(…) {` etc.
pub fn symbol_of(r: &SearchResult) -> Option<String> {
    if let Some(sig) = continued_signature(&r.chunk) {
        return Some(format!(
            "{} (cont.)",
            clip(sig.trim().trim_end_matches('{').trim_end(), 100)
        ));
    }
    source_lines(r)
        .into_iter()
        .map(|(_, l)| l.trim())
        .filter(|l| {
            !l.is_empty() && !l.starts_with("//") && !l.starts_with('#') && !l.starts_with('@')
        })
        .take(12)
        .find(|l| DEF_PREFIXES.iter().any(|p| l.starts_with(p)))
        .map(|l| clip(l.trim_end_matches('{').trim_end(), 100))
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "that", "this", "how", "what", "where", "which", "does",
    "into", "are", "its", "una", "los", "las", "del", "con", "para", "que", "como", "por",
];

fn query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = query
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 3 && !STOPWORDS.contains(&t.as_str()))
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

/// Up to `max` lines of the chunk worth showing for `query`: the ones that
/// contain the most query terms (kept in source order); without any match,
/// the first lines that are not imports or blank.
pub fn preview(r: &SearchResult, query: &str, max: usize) -> Vec<(usize, String)> {
    let lines = source_lines(r);
    let terms = query_terms(query);
    let mut scored: Vec<(usize, usize)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, (_, l))| {
            let low = l.to_lowercase();
            let hits = terms.iter().filter(|t| low.contains(t.as_str())).count();
            (hits > 0 && !l.trim().is_empty()).then_some((i, hits))
        })
        .collect();
    let picked: Vec<usize> = if scored.is_empty() {
        lines
            .iter()
            .enumerate()
            .filter(|(_, (_, l))| {
                let t = l.trim();
                !t.is_empty() && !super::search::is_import_line_pub(t)
            })
            .map(|(i, _)| i)
            .take(max)
            .collect()
    } else {
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut idx: Vec<usize> = scored.into_iter().take(max).map(|(i, _)| i).collect();
        idx.sort_unstable();
        idx
    };
    picked
        .into_iter()
        .map(|i| (lines[i].0, clip(lines[i].1.trim_end(), MAX_LINE_CHARS)))
        .collect()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// `path:L10-L58`, the location agents pass back to `get_chunk`.
pub fn location(r: &SearchResult) -> String {
    format!("{}:L{}-L{}", r.file_path, r.line_start, r.line_end)
}

/// One hit, compact.
pub fn render_compact(rank: usize, r: &SearchResult, query: &str) -> String {
    let mut out = format!("## {}. {} · {:.2}", rank, location(r), r.score);
    let kind = chunk_kind(&r.file_path, &r.chunk);
    if kind != ChunkKind::Code {
        out.push_str(&format!(" · {}", kind.as_str()));
    }
    if let Some(sym) = symbol_of(r) {
        out.push_str(&format!(" · `{}`", sym));
    }
    out.push('\n');
    for (n, line) in preview(r, query, PREVIEW_LINES) {
        out.push_str(&format!("{:>5}│ {}\n", n, line));
    }
    out
}

/// One hit, whole chunk (the pre-0.35 format).
pub fn render_full(rank: usize, r: &SearchResult) -> String {
    format!(
        "## {}. {} (score: {:.2}, lines {}-{})\n\n```\n{}\n```\n",
        rank, r.file_path, r.score, r.line_start, r.line_end, r.chunk
    )
}

/// A hit already sent earlier in this session: location only.
pub fn render_seen(rank: usize, r: &SearchResult) -> String {
    format!(
        "## {}. {} · {:.2} · ↺ already sent this session\n",
        rank,
        location(r),
        r.score
    )
}

/// Render a result list. `seen(i)` marks hits sent before in the session
/// (printed as a one-line reference).
pub fn render_results(
    results: &[SearchResult],
    query: &str,
    detail: Detail,
    seen: impl Fn(&SearchResult) -> bool,
) -> String {
    let mut out = String::new();
    let mut any_compact = false;
    let mut any_seen = false;
    for (i, r) in results.iter().enumerate() {
        let rank = i + 1;
        if seen(r) {
            out.push_str(&render_seen(rank, r));
            any_seen = true;
            continue;
        }
        let full = match detail {
            Detail::Full => true,
            Detail::Compact => false,
            Detail::Auto => i == 0 && r.score >= AUTO_FULL_SCORE,
        };
        if full {
            out.push_str(&render_full(rank, r));
        } else {
            out.push_str(&render_compact(rank, r, query));
            any_compact = true;
        }
        out.push('\n');
    }
    if any_compact {
        out.push_str(
            "Expand a hit with get_chunk(project, file_path, line_start, line_end), or search with detail=\"full\".\n",
        );
    }
    if any_seen {
        out.push_str(
            "↺ hits were already returned earlier; pass include_seen=true to resend them.\n",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str, start: usize, text: &str, score: f32) -> SearchResult {
        SearchResult {
            file_path: path.into(),
            chunk: text.into(),
            score,
            line_start: start,
            line_end: start + text.lines().count().saturating_sub(1),
            community_id: None,
        }
    }

    const CHUNK: &str = "// Used by: server\n// Imports: db\n// src/search.rs\npub fn search_with_mode(project: &Project, query: &str) -> Vec<Hit> {\n    let conn = open(project);\n    let fused = rrf_fuse(&vector, &lexical);\n    let x = 1;\n    let y = 2;\n    let z = 3;\n    fused.into_iter().take(5).collect()\n}\n";

    #[test]
    fn symbol_skips_context_header() {
        let r = hit("src/search.rs", 100, CHUNK, 0.8);
        assert_eq!(
            symbol_of(&r).as_deref(),
            Some("pub fn search_with_mode(project: &Project, query: &str) -> Vec<Hit>")
        );
    }

    #[test]
    fn preview_prefers_query_terms_with_real_line_numbers() {
        let r = hit("src/search.rs", 100, CHUNK, 0.8);
        let p = preview(&r, "rrf fuse lexical vector", 1);
        assert_eq!(p.len(), 1);
        // Line 100 is the fn signature (header lines are not in the file);
        // the line matching the most terms wins.
        assert_eq!(p[0].0, 102);
        assert!(p[0].1.contains("rrf_fuse"));
        // With room for two, the weaker `fused` match joins, in source order.
        let p = preview(&r, "rrf fuse lexical vector", 2);
        assert_eq!(p.iter().map(|x| x.0).collect::<Vec<_>>(), vec![102, 106]);
    }

    #[test]
    fn preview_without_matches_shows_first_code_lines() {
        let r = hit(
            "a.rs",
            1,
            "use std::fs;\n\nfn main() {\n    run();\n}\n",
            0.5,
        );
        let p = preview(&r, "zzzz qqqq", 2);
        assert_eq!(p[0], (3, "fn main() {".to_string()));
        assert_eq!(p[1].0, 4);
    }

    #[test]
    fn compact_is_much_smaller_than_full() {
        let big: String = (0..80)
            .map(|i| format!("    let v{i} = compute_{i}(input);\n"))
            .collect();
        let text = format!("fn pipeline(input: &str) {{\n{big}}}\n");
        let r = hit("src/p.rs", 10, &text, 0.7);
        let c = render_compact(1, &r, "compute pipeline input");
        let f = render_full(1, &r);
        assert!(
            c.len() * 5 < f.len(),
            "compact {} vs full {}",
            c.len(),
            f.len()
        );
        assert!(c.starts_with("## 1. src/p.rs:L10-L91 · 0.70 · `fn pipeline(input: &str)`"));
    }

    #[test]
    fn auto_expands_only_a_confident_top_hit() {
        let a = hit("a.rs", 1, "fn a() {\n    one();\n}\n", 0.9);
        let b = hit("b.rs", 1, "fn b() {\n    two();\n}\n", 0.9);
        let out = render_results(&[a.clone(), b], "one two", Detail::Auto, |_| false);
        assert!(
            out.contains("(score: 0.90, lines 1-3)"),
            "top hit full: {out}"
        );
        assert!(out.contains("## 2. b.rs:L1-L3"), "second compact: {out}");
        let low = hit("a.rs", 1, "fn a() {}\n", 0.5);
        let out = render_results(&[low], "a", Detail::Auto, |_| false);
        assert!(!out.contains("```"));
    }

    #[test]
    fn seen_hits_are_one_line_references() {
        let a = hit("CLAUDE.md", 379, &"text line\n".repeat(30), 0.6);
        let out = render_results(&[a], "x", Detail::Compact, |_| true);
        assert!(out.starts_with("## 1. CLAUDE.md:L379-L408 · 0.60 · ↺ already sent"));
        assert!(out.len() < 200);
        assert!(out.contains("include_seen=true"));
    }

    #[test]
    fn continued_chunks_name_their_function() {
        let text = "// src/big.rs\n// (continued) fn big_one() {\n    step_40();\n    step_41();\n";
        let r = hit("src/big.rs", 41, text, 0.6);
        assert_eq!(symbol_of(&r).as_deref(), Some("fn big_one() (cont.)"));
        let p = preview(&r, "step", 5);
        assert_eq!(p[0], (41, "    step_40();".to_string()));
    }

    #[test]
    fn detail_parse_defaults_to_compact() {
        assert_eq!(Detail::parse(None), Detail::Compact);
        assert_eq!(Detail::parse(Some("FULL")), Detail::Full);
        assert_eq!(Detail::parse(Some("auto")), Detail::Auto);
        assert_eq!(Detail::parse(Some("whatever")), Detail::Compact);
    }
}
