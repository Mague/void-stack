use serde::Serialize;
use tauri::State;

use void_stack_core::audit;
use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct SecurityFindingDto {
    pub id: String,
    pub severity: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub remediation: String,
}

#[derive(Serialize)]
pub struct AuditSummaryDto {
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub info: u32,
    pub total: u32,
    pub risk_score: f32,
}

#[derive(Serialize)]
pub struct AuditResultDto {
    pub project_name: String,
    pub timestamp: String,
    pub findings: Vec<SecurityFindingDto>,
    pub summary: AuditSummaryDto,
}

#[tauri::command]
pub async fn run_security_audit(
    project: String,
    _state: State<'_, AppState>,
) -> Result<AuditResultDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let clean_path = strip_win_prefix(&proj.path);

    let result = tokio::task::spawn_blocking(move || {
        audit::audit_project(&project, std::path::Path::new(&clean_path))
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(AuditResultDto {
        project_name: result.project_name,
        timestamp: result.timestamp,
        findings: result
            .findings
            .iter()
            .map(|f| SecurityFindingDto {
                id: f.id.clone(),
                severity: f.severity.to_string(),
                category: f.category.to_string(),
                title: f.title.clone(),
                description: f.description.clone(),
                file_path: f.file_path.clone(),
                line_number: f.line_number,
                remediation: f.remediation.clone(),
            })
            .collect(),
        summary: AuditSummaryDto {
            critical: result.summary.critical,
            high: result.summary.high,
            medium: result.summary.medium,
            low: result.summary.low,
            info: result.summary.info,
            total: result.summary.total,
            risk_score: result.summary.risk_score,
        },
    })
}
