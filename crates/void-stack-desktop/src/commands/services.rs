use serde::Serialize;
use tauri::State;

use void_stack_core::global_config::load_global_config;
use void_stack_core::model::ServiceStatus;

use crate::state::AppState;

#[derive(Serialize, Clone)]
pub struct ServiceStateDto {
    pub service_name: String,
    pub status: String,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub url: Option<String>,
}

fn states_to_dto(states: &[void_stack_core::model::ServiceState]) -> Vec<ServiceStateDto> {
    states.iter().map(|state| ServiceStateDto {
        service_name: state.service_name.clone(),
        status: match state.status {
            ServiceStatus::Running => "RUNNING",
            ServiceStatus::Stopped => "STOPPED",
            ServiceStatus::Starting => "STARTING",
            ServiceStatus::Failed => "FAILED",
            ServiceStatus::Stopping => "STOPPING",
        }.to_string(),
        pid: state.pid,
        started_at: state.started_at.map(|dt| dt.to_rfc3339()),
        url: state.url.clone(),
    }).collect()
}

#[tauri::command]
pub async fn get_project_status(project: String, state: State<'_, AppState>) -> Result<Vec<ServiceStateDto>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let mgr = state.get_manager(&proj).await;
    let _ = mgr.refresh_status().await;
    let states = mgr.get_states().await;
    Ok(states_to_dto(&states))
}

#[tauri::command]
pub async fn start_all(project: String, state: State<'_, AppState>) -> Result<Vec<ServiceStateDto>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let mgr = state.get_manager(&proj).await;
    mgr.start_all().await.map_err(|e| e.to_string())?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let _ = mgr.refresh_status().await;
    let states = mgr.get_states().await;
    Ok(states_to_dto(&states))
}

#[tauri::command]
pub async fn stop_all(project: String, state: State<'_, AppState>) -> Result<(), String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let mgr = state.get_manager(&proj).await;
    mgr.stop_all().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn start_service(project: String, service: String, state: State<'_, AppState>) -> Result<ServiceStateDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let mgr = state.get_manager(&proj).await;
    mgr.start_one(&service).await.map_err(|e| e.to_string())?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let _ = mgr.refresh_status().await;
    let states = mgr.get_states().await;
    let dto = states_to_dto(&states);
    dto.into_iter()
        .find(|s| s.service_name == service)
        .ok_or_else(|| format!("Servicio '{}' no encontrado", service))
}

#[tauri::command]
pub async fn stop_service(project: String, service: String, state: State<'_, AppState>) -> Result<(), String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let mgr = state.get_manager(&proj).await;
    mgr.stop_one(&service).await.map_err(|e| e.to_string())?;
    Ok(())
}
