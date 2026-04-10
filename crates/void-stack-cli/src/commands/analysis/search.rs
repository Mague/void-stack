use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};

pub fn cmd_index(project_name: &str, force: bool) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    println!("Indexing {}...\n", project.name);

    let stats = void_stack_core::vector_index::index_project(project, force, |done, total| {
        // Simple progress bar
        let pct = if total > 0 { done * 100 / total } else { 0 };
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
        println!("\n{}. {} ({:.2})", i + 1, r.file_path, r.score);
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
