use serde::Serialize;
use tauri::State;

use void_stack_core::detector;
use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct DependencyStatusDto {
    pub dep_type: String,
    pub status: String,
    pub version: Option<String>,
    pub details: Vec<String>,
    pub fix_hint: Option<String>,
}

#[tauri::command]
pub async fn check_dependencies(
    project: String,
    _state: State<'_, AppState>,
) -> Result<Vec<DependencyStatusDto>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let mut dirs = vec![strip_win_prefix(&proj.path)];
    for svc in &proj.services {
        if let Some(d) = &svc.working_dir {
            let clean = strip_win_prefix(d);
            if !dirs.contains(&clean) {
                dirs.push(clean);
            }
        }
    }

    let mut results = Vec::new();
    for dir in &dirs {
        let path = std::path::Path::new(dir);
        let statuses = detector::check_project(path).await;
        for dep_status in statuses {
            // Avoid duplicates by dep_type
            if !results
                .iter()
                .any(|r: &DependencyStatusDto| r.dep_type == format!("{:?}", dep_status.dep_type))
            {
                results.push(DependencyStatusDto {
                    dep_type: format!("{:?}", dep_status.dep_type),
                    status: format!("{:?}", dep_status.status),
                    version: dep_status.version,
                    details: dep_status.details,
                    fix_hint: dep_status.fix_hint,
                });
            }
        }
    }

    Ok(results)
}
