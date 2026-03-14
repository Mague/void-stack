use serde::Serialize;
use tauri::State;

use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;
use void_stack_core::space;

use crate::state::AppState;

#[derive(Serialize)]
pub struct SpaceEntryDto {
    pub name: String,
    pub category: String,
    pub path: String,
    pub size_bytes: u64,
    pub size_human: String,
    pub deletable: bool,
    pub restore_hint: String,
}

fn to_dto(entry: &space::SpaceEntry) -> SpaceEntryDto {
    SpaceEntryDto {
        name: entry.name.clone(),
        category: format!("{}", entry.category),
        path: entry.path.clone(),
        size_bytes: entry.size_bytes,
        size_human: entry.size_human.clone(),
        deletable: entry.deletable,
        restore_hint: entry.restore_hint.clone(),
    }
}

/// Scan a specific project for heavy directories.
#[tauri::command]
pub async fn scan_project_space(
    project: String,
    _state: State<'_, AppState>,
) -> Result<Vec<SpaceEntryDto>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let path = strip_win_prefix(&proj.path);

    // Run in blocking task since dir_size is CPU+IO intensive
    let entries =
        tokio::task::spawn_blocking(move || space::scan_project(std::path::Path::new(&path)))
            .await
            .map_err(|e| e.to_string())?;

    Ok(entries.iter().map(to_dto).collect())
}

/// Scan global caches and AI model storage.
#[tauri::command]
pub async fn scan_global_space() -> Result<Vec<SpaceEntryDto>, String> {
    let entries = tokio::task::spawn_blocking(space::scan_global)
        .await
        .map_err(|e| e.to_string())?;

    Ok(entries.iter().map(to_dto).collect())
}

/// Delete a specific directory to free space.
#[tauri::command]
pub async fn delete_space_entry(path: String) -> Result<String, String> {
    let freed = tokio::task::spawn_blocking(move || space::delete_entry(&path))
        .await
        .map_err(|e| e.to_string())??;

    // Format freed size
    let human = if freed >= 1_073_741_824 {
        format!("{:.1} GB", freed as f64 / 1_073_741_824.0)
    } else if freed >= 1_048_576 {
        format!("{:.1} MB", freed as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", freed as f64 / 1024.0)
    };

    Ok(format!("{} liberados", human))
}
