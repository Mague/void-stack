use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};

pub fn cmd_index(project_name: &str, force: bool, git_base: Option<&str>) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    println!("Indexing {}...\n", project.name);

    let stats =
        void_stack_core::vector_index::index_project(project, force, git_base, |done, total| {
            // Simple progress bar
            let pct = (done * 100).checked_div(total).unwrap_or(0);
            let filled = pct / 5;
            let empty = 20 - filled;
            eprint!(
                "\r[{}{}] {}/{} archivos",
                "█".repeat(filled),
                "░".repeat(empty),
                done,
                total
            );
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    eprintln!();
    println!(
        "\n✓ Index creado: {} archivos, {} chunks ({:.1}MB)",
        stats.files_indexed, stats.chunks_total, stats.size_mb
    );
    Ok(())
}

pub fn cmd_generate_voidignore(project_name: &str) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let project_path = std::path::Path::new(&project.path);
    let result = void_stack_core::vector_index::generate_voidignore(project_path);
    let path = void_stack_core::vector_index::save_voidignore(project_path, &result.content)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!(
        "Generated .voidignore ({} patterns) → {}",
        result.patterns_count,
        path.display()
    );
    Ok(())
}

pub fn cmd_search(project_name: &str, query: &str, top_k: usize) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let results = void_stack_core::vector_index::semantic_search(project, query, top_k)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if results.is_empty() {
        println!("No results found for: \"{}\"", query);
        return Ok(());
    }

    for (i, r) in results.iter().enumerate() {
        let community = match r.community_id {
            Some(c) => format!(" [community {}]", c),
            None => String::new(),
        };
        println!("\n{}. {} ({:.2}){}", i + 1, r.file_path, r.score, community);
        println!("   líneas {}-{}", r.line_start, r.line_end);

        // Show first 5 lines of chunk (skip the file path comment)
        let preview: Vec<&str> = r.chunk.lines().skip(1).take(5).collect();
        for line in preview {
            println!("   {}", line);
        }
        if r.chunk.lines().count() > 6 {
            println!("   ...");
        }
    }

    println!();
    Ok(())
}

pub fn cmd_cluster(project_name: &str) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    println!("Clustering {}...", project.name);

    let stats = void_stack_core::vector_index::cluster_project(project)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!(
        "Clustered {} chunks into {} communities (largest: {} members)",
        stats.chunks_total, stats.communities, stats.largest_community_size
    );
    Ok(())
}

#[cfg(feature = "structural")]
pub fn cmd_review(project_name: &str, git_base: Option<&str>) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;
    let payload = void_stack_core::review::review_diff(project, git_base)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("{}", payload.markdown);
    Ok(())
}

pub fn cmd_suggest_tests(project_name: &str, git_base: Option<&str>) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;
    let suggestions = void_stack_core::testing::suggest_tests_for_diff(project, git_base, 20)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!(
        "{}",
        void_stack_core::testing::render_suggestions_markdown(&suggestions)
    );
    Ok(())
}

pub fn cmd_graph_build(project_name: &str, force: bool) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;
    let stats = void_stack_core::structural::build_structural_graph(project, force)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!(
        "Structural graph for '{}' built:\n  files_parsed:  {}\n  files_skipped: {}\n  nodes_total:   {}\n  edges_total:   {}",
        project.name, stats.files_parsed, stats.files_skipped, stats.nodes_total, stats.edges_total,
    );
    Ok(())
}

#[cfg(all(feature = "vector", feature = "structural"))]
pub fn cmd_graphrag(project_name: &str, query: &str, depth: u8) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let result = void_stack_core::vector_index::graph_rag_search(project, query, 5, depth)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("\n## Semantic Seeds ({})", result.semantic_seeds.len());
    for (i, r) in result.semantic_seeds.iter().enumerate() {
        let community = match r.community_id {
            Some(c) => format!(" [community {}]", c),
            None => String::new(),
        };
        println!(
            "  {}. {} (score {:.2}, lines {}-{}){}",
            i + 1,
            r.file_path,
            r.score,
            r.line_start,
            r.line_end,
            community
        );
    }

    println!(
        "\n## Structural Context ({})",
        result.structural_context.len()
    );
    for (i, c) in result.structural_context.iter().enumerate() {
        let src = match c.source {
            void_stack_core::vector_index::ContextSource::Caller => "caller",
            void_stack_core::vector_index::ContextSource::Callee => "callee",
            void_stack_core::vector_index::ContextSource::TestFor => "test",
        };
        println!(
            "  {}. [{}/hop {}] {} (lines {}-{})",
            i + 1,
            src,
            c.hops,
            c.file_path,
            c.line_start,
            c.line_end
        );
    }

    println!(
        "\n~{} tokens | {} semantic + {} structural chunks",
        result.token_estimate,
        result.semantic_seeds.len(),
        result.structural_context.len()
    );
    Ok(())
}

#[cfg(all(feature = "vector", feature = "structural"))]
pub fn cmd_graphrag_cross(project_name: &str, query: &str, depth: u8) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();

    let result = void_stack_core::vector_index::graph_rag_search_cross(
        &config, &project, query, 5, depth, None,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let primary = &result.primary;
    println!(
        "\n## Primary: {} — Semantic Seeds ({})",
        project.name,
        primary.semantic_seeds.len()
    );
    for (i, r) in primary.semantic_seeds.iter().enumerate() {
        println!(
            "  {}. {} (score {:.2}, lines {}-{})",
            i + 1,
            r.file_path,
            r.score,
            r.line_start,
            r.line_end
        );
    }

    if !result.related.is_empty() {
        println!("\n## Related Projects");
        for (proj_name, hits) in &result.related {
            let via = result
                .cross_links
                .iter()
                .find(|l| l.to_project.eq_ignore_ascii_case(proj_name))
                .map(|l| l.via.as_str())
                .unwrap_or("no shared symbols");
            println!("\n### {} (via: {})", proj_name, via);
            for (i, r) in hits.iter().enumerate() {
                println!(
                    "  {}. {} (score {:.2}, lines {}-{})",
                    i + 1,
                    r.file_path,
                    r.score,
                    r.line_start,
                    r.line_end
                );
            }
        }
    }

    if !result.cross_links.is_empty() {
        println!("\n## Shared Symbols");
        for link in &result.cross_links {
            println!(
                "  {} → {} (via {}): {}",
                link.from_project,
                link.to_project,
                link.via,
                link.shared_symbols.join(", ")
            );
        }
    }

    println!(
        "\n~{} tokens | primary: {} seeds + {} structural | related: {} projects",
        primary.token_estimate,
        primary.semantic_seeds.len(),
        primary.structural_context.len(),
        result.related.len(),
    );
    Ok(())
}
