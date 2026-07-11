#[cfg(feature = "vector")]
use void_stack_core::global_config::load_global_config;

#[cfg(feature = "vector")]
use crate::state::AppState;

#[tauri::command]
pub async fn index_project_codebase_cmd(
    project_name: String,
    force: bool,
) -> Result<String, String> {
    #[cfg(feature = "vector")]
    {
        // Indexing embeds the whole codebase — minutes, not millis; a
        // sync command here would freeze the UI thread for the duration.
        blocking(move || {
            let config = load_global_config().map_err(|e| e.to_string())?;
            let proj = AppState::find_project(&config, &project_name)?;
            let stats = void_stack_core::vector_index::index_project(&proj, force, None, |_, _| {})
                .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&stats).map_err(|e| e.to_string())
        })
        .await
    }
    #[cfg(not(feature = "vector"))]
    {
        let _ = (project_name, force);
        Err("Vector search not available. Rebuild with --features vector".to_string())
    }
}

/// Run a closure on a blocking thread so a multi-second search never freezes
/// the UI thread.
#[cfg(feature = "vector")]
async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("task join error: {}", e))?
}

#[tauri::command]
pub async fn semantic_search_cmd(
    project_name: String,
    query: String,
    top_k: Option<usize>,
) -> Result<String, String> {
    #[cfg(feature = "vector")]
    {
        let config = load_global_config().map_err(|e| e.to_string())?;
        let proj = AppState::find_project(&config, &project_name)?;
        blocking(move || {
            let results =
                void_stack_core::vector_index::semantic_search(&proj, &query, top_k.unwrap_or(5))
                    .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
        })
        .await
    }
    #[cfg(not(feature = "vector"))]
    {
        let _ = (query, top_k);
        Err("Vector search not available. Rebuild with --features vector".to_string())
    }
}

/// GraphRAG: semantic seeds expanded with structural call-graph context.
#[tauri::command]
pub async fn graph_rag_search_cmd(
    project_name: String,
    query: String,
    top_k: Option<usize>,
    depth: Option<u8>,
) -> Result<String, String> {
    #[cfg(feature = "vector")]
    {
        let config = load_global_config().map_err(|e| e.to_string())?;
        let proj = AppState::find_project(&config, &project_name)?;
        blocking(move || {
            let res = void_stack_core::vector_index::graph_rag_search(
                &proj,
                &query,
                top_k.unwrap_or(8),
                depth.unwrap_or(2),
            )
            .map_err(|e| e.to_string())?;
            serde_json::to_string(&res).map_err(|e| e.to_string())
        })
        .await
    }
    #[cfg(not(feature = "vector"))]
    {
        let _ = (query, top_k, depth);
        Err("Vector search not available. Rebuild with --features vector".to_string())
    }
}

/// Cross-project GraphRAG: the primary project's result plus matches and
/// inferred links in related projects (API contracts, shared symbols).
/// `related` restricts the search to the chosen projects; `None`/empty means
/// auto-detect across all registered projects.
#[tauri::command]
pub async fn graph_rag_search_cross_cmd(
    project_name: String,
    query: String,
    top_k: Option<usize>,
    depth: Option<u8>,
    related: Option<Vec<String>>,
) -> Result<String, String> {
    #[cfg(feature = "vector")]
    {
        let config = load_global_config().map_err(|e| e.to_string())?;
        let proj = AppState::find_project(&config, &project_name)?;
        blocking(move || {
            let related_ref = related.as_deref().filter(|r| !r.is_empty());
            let res = void_stack_core::vector_index::graph_rag_search_cross(
                &config,
                &proj,
                &query,
                top_k.unwrap_or(8),
                depth.unwrap_or(2),
                related_ref,
            )
            .map_err(|e| e.to_string())?;
            serde_json::to_string(&res).map_err(|e| e.to_string())
        })
        .await
    }
    #[cfg(not(feature = "vector"))]
    {
        let _ = (query, top_k, depth, related);
        Err("Vector search not available. Rebuild with --features vector".to_string())
    }
}

#[tauri::command]
pub fn generate_voidignore_cmd(project_name: String) -> Result<String, String> {
    #[cfg(feature = "vector")]
    {
        let config = load_global_config().map_err(|e| e.to_string())?;
        let proj = AppState::find_project(&config, &project_name)?;
        let project_path = std::path::Path::new(&proj.path);
        let result = void_stack_core::vector_index::generate_voidignore(project_path);
        void_stack_core::vector_index::save_voidignore(project_path, &result.content)
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "{{\"patterns_count\":{},\"content\":{}}}",
            result.patterns_count,
            serde_json::to_string(&result.content).map_err(|e| e.to_string())?
        ))
    }
    #[cfg(not(feature = "vector"))]
    Err("Vector search not available. Rebuild with --features vector".to_string())
}

#[tauri::command]
pub fn get_index_stats_cmd(project_name: String) -> Result<String, String> {
    #[cfg(feature = "vector")]
    {
        let config = load_global_config().map_err(|e| e.to_string())?;
        let proj = AppState::find_project(&config, &project_name)?;
        match void_stack_core::vector_index::get_index_stats(&proj) {
            Ok(Some(stats)) => serde_json::to_string_pretty(&stats).map_err(|e| e.to_string()),
            Ok(None) => Ok("null".to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(not(feature = "vector"))]
    Err("Vector search not available. Rebuild with --features vector".to_string())
}
