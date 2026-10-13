//! `void map`, `void entrypoints` and `void bench`: the project inventory
//! tools, and a reproducible measurement of what search costs in tokens.

use anyhow::Result;
use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::model::Project;

fn project(name: &str) -> Result<Project> {
    let config = load_global_config()?;
    Ok(find_project(&config, name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", name))?
        .clone())
}

fn root(p: &Project) -> std::path::PathBuf {
    std::path::PathBuf::from(void_stack_core::runner::local::strip_win_prefix(&p.path))
}

/// `void map <project>`: packages, entry files, tree and key docs.
pub fn cmd_map(project_name: &str, depth: usize) -> Result<()> {
    let p = project(project_name)?;
    print!(
        "{}",
        void_stack_core::inventory::project_map(&p.name, &root(&p), depth)
    );
    Ok(())
}

/// `void entrypoints <project> [--kind rest,cli]`.
pub fn cmd_entrypoints(project_name: &str, kind: Option<&str>, max: usize) -> Result<()> {
    let p = project(project_name)?;
    let kinds: Vec<_> = kind
        .unwrap_or("")
        .split(',')
        .filter_map(void_stack_core::inventory::EntryKind::parse)
        .collect();
    let entries = void_stack_core::inventory::entrypoints(&p, &kinds);
    print!(
        "{}",
        void_stack_core::inventory::render_entrypoints(&entries, max)
    );
    Ok(())
}

/// One measured search.
#[cfg(feature = "vector")]
struct BenchRow {
    query: String,
    ms: u128,
    results: usize,
    /// Tokens (chars/4) the compact output costs.
    compact_tokens: usize,
    /// Tokens the full-chunk output costs (pre-0.35 format).
    full_tokens: usize,
    /// Tokens of reading every file the hits came from, whole.
    files_tokens: usize,
    /// Hits that are tests, docs or changelogs.
    secondary: usize,
}

/// Queries of a task file: one per line, `#` comments and blanks skipped.
pub(crate) fn parse_task(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// `void bench <project> --task <file>`: run each query of the task file
/// and report what it costs in tokens — compact output, full chunks, and
/// reading the source files whole — plus latency. Reads the index only.
#[cfg(feature = "vector")]
pub fn cmd_bench(project_name: &str, task: &str, top_k: usize, json: bool) -> Result<()> {
    use void_stack_core::vector_index::{ChunkKind, Detail, SearchMode, chunk_kind, render};

    let p = project(project_name)?;
    let text = std::fs::read_to_string(task)
        .map_err(|e| anyhow::anyhow!("cannot read task file {}: {}", task, e))?;
    let queries = parse_task(&text);
    if queries.is_empty() {
        anyhow::bail!("task file {} has no queries", task);
    }
    let root = root(&p);
    let tok = |s: usize| s.div_ceil(4);
    let mut rows = Vec::new();
    for q in &queries {
        let t0 = std::time::Instant::now();
        let results =
            void_stack_core::vector_index::search_unrecorded(&p, q, top_k, SearchMode::Hybrid)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        let ms = t0.elapsed().as_millis();
        let compact = render::render_results(&results, q, Detail::Compact, |_| false);
        let full = render::render_results(&results, q, Detail::Full, |_| false);
        let mut files: Vec<&str> = results.iter().map(|r| r.file_path.as_str()).collect();
        files.sort_unstable();
        files.dedup();
        let files_bytes: u64 = files
            .iter()
            .filter_map(|f| std::fs::metadata(root.join(f)).ok())
            .map(|m| m.len())
            .sum();
        rows.push(BenchRow {
            query: q.clone(),
            ms,
            results: results.len(),
            compact_tokens: tok(compact.len()),
            full_tokens: tok(full.len()),
            files_tokens: tok(files_bytes as usize),
            secondary: results
                .iter()
                .filter(|r| chunk_kind(&r.file_path, &r.chunk) != ChunkKind::Code)
                .count(),
        });
    }

    let map = void_stack_core::inventory::project_map(&p.name, &root, 3);
    let entries = void_stack_core::inventory::entrypoints(&p, &[]);
    let entries_text = void_stack_core::inventory::render_entrypoints(&entries, 200);

    if json {
        let out = serde_json::json!({
            "project": p.name,
            "top_k": top_k,
            "queries": rows.iter().map(|r| serde_json::json!({
                "query": r.query,
                "ms": r.ms as u64,
                "results": r.results,
                "compact_tokens": r.compact_tokens,
                "full_tokens": r.full_tokens,
                "files_tokens": r.files_tokens,
                "secondary": r.secondary,
            })).collect::<Vec<_>>(),
            "project_map_tokens": tok(map.len()),
            "entrypoints": entries.len(),
            "entrypoints_tokens": tok(entries_text.len()),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!(
        "# void bench — {} ({} queries, top_k={})\n",
        p.name,
        rows.len(),
        top_k
    );
    println!("| query | ms | hits | compact tok | full tok | whole files tok | tests/docs |");
    println!("|---|---:|---:|---:|---:|---:|---:|");
    for r in &rows {
        let q: String = r.query.chars().take(48).collect();
        println!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            q, r.ms, r.results, r.compact_tokens, r.full_tokens, r.files_tokens, r.secondary
        );
    }
    let sum = |f: fn(&BenchRow) -> usize| rows.iter().map(f).sum::<usize>();
    let (c, f, w) = (
        sum(|r| r.compact_tokens),
        sum(|r| r.full_tokens),
        sum(|r| r.files_tokens),
    );
    let ms: u128 = rows.iter().map(|r| r.ms).sum();
    println!(
        "| **total** | {} | {} | **{}** | {} | {} | {} |",
        ms,
        sum(|r| r.results),
        c,
        f,
        w,
        sum(|r| r.secondary)
    );
    let ratio = |a: usize, b: usize| if a == 0 { 0.0 } else { b as f64 / a as f64 };
    println!(
        "\ncompact is {:.1}× smaller than full chunks and {:.1}× smaller than reading the hit files whole.",
        ratio(c, f),
        ratio(c, w)
    );
    println!(
        "project_map: {} tok · list_entrypoints: {} entry points in {} tok",
        tok(map.len()),
        entries.len(),
        tok(entries_text.len())
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_file_skips_comments_and_blanks() {
        let t = "# inventory\nmain features\n\n  cli commands  \n# end\n";
        assert_eq!(parse_task(t), vec!["main features", "cli commands"]);
    }
}
