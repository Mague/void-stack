use serde::Serialize;

use void_stack_core::analyzer;
use void_stack_core::analyzer::history;
use void_stack_core::analyzer::patterns::antipatterns::AntiPatternKind;
use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct SnapshotDto {
    pub timestamp: String,
    pub label: Option<String>,
    pub services: Vec<ServiceSnapshotDto>,
}

#[derive(Serialize)]
pub struct ServiceSnapshotDto {
    pub name: String,
    pub pattern: String,
    pub total_modules: usize,
    pub total_loc: usize,
    pub anti_pattern_count: usize,
    pub avg_complexity: f32,
    pub max_complexity: usize,
    pub complex_functions: usize,
    pub coverage_percent: Option<f32>,
    pub god_classes: usize,
    pub circular_deps: usize,
    // Detail fields (populated only for live analysis, None for history)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub god_classes_detail: Option<Vec<GodClassDetailDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complex_functions_detail: Option<Vec<ComplexFunctionDetailDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anti_patterns_detail: Option<Vec<AntiPatternDetailDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circular_deps_detail: Option<Vec<CircularDepDetailDto>>,
}

#[derive(Serialize)]
pub struct GodClassDetailDto {
    pub file: String,
    pub loc: usize,
    pub functions: usize,
    pub severity: String,
}

#[derive(Serialize)]
pub struct ComplexFunctionDetailDto {
    pub file: String,
    pub name: String,
    pub line: usize,
    pub complexity: usize,
}

#[derive(Serialize)]
pub struct AntiPatternDetailDto {
    pub kind: String,
    pub description: String,
    pub affected: Vec<String>,
    pub severity: String,
    pub suggestion: String,
}

#[derive(Serialize)]
pub struct CircularDepDetailDto {
    pub cycle: Vec<String>,
}

#[derive(Serialize)]
pub struct DebtComparisonDto {
    pub previous: String,
    pub current: String,
    pub overall_trend: String,
    pub services: Vec<ServiceComparisonDto>,
}

#[derive(Serialize)]
pub struct ServiceComparisonDto {
    pub name: String,
    pub loc_delta: i64,
    pub antipattern_delta: i32,
    pub complexity_delta: f32,
    pub coverage_delta: Option<f32>,
    pub god_class_delta: i32,
    pub circular_dep_delta: i32,
    pub trend: String,
}

fn snap_to_dto(s: &history::AnalysisSnapshot) -> SnapshotDto {
    SnapshotDto {
        timestamp: s.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
        label: s.label.clone(),
        services: s.services.iter().map(|svc| ServiceSnapshotDto {
            name: svc.name.clone(),
            pattern: svc.pattern.clone(),
            total_modules: svc.total_modules,
            total_loc: svc.total_loc,
            anti_pattern_count: svc.anti_pattern_count,
            avg_complexity: svc.avg_complexity,
            max_complexity: svc.max_complexity,
            complex_functions: svc.complex_functions,
            coverage_percent: svc.coverage_percent,
            god_classes: svc.god_classes,
            circular_deps: svc.circular_deps,
            god_classes_detail: None,
            complex_functions_detail: None,
            anti_patterns_detail: None,
            circular_deps_detail: None,
        }).collect(),
    }
}

/// Build enriched DTO with detail from live analysis results.
fn enriched_dto(results: &[(String, analyzer::AnalysisResult)]) -> SnapshotDto {
    let snapshot = history::create_snapshot(results, None);
    SnapshotDto {
        timestamp: snapshot.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
        label: None,
        services: snapshot.services.iter().zip(results.iter()).map(|(svc_snap, (_name, result))| {
            let anti_patterns = &result.architecture.anti_patterns;

            let god_classes_detail: Vec<GodClassDetailDto> = anti_patterns.iter()
                .filter(|a| a.kind == AntiPatternKind::GodClass)
                .map(|a| {
                    let file = a.affected_modules.first().cloned().unwrap_or_default();
                    // Extract LOC and function count from the graph
                    let (loc, fns) = result.graph.modules.iter()
                        .find(|m| m.path == file)
                        .map(|m| (m.loc, m.function_count))
                        .unwrap_or((0, 0));
                    GodClassDetailDto {
                        file,
                        loc,
                        functions: fns,
                        severity: format!("{}", a.severity),
                    }
                })
                .collect();

            let mut complex_functions_detail: Vec<ComplexFunctionDetailDto> = Vec::new();
            if let Some(cx) = &result.complexity {
                for (file, fc) in cx {
                    for func in &fc.functions {
                        if func.complexity >= 10 {
                            complex_functions_detail.push(ComplexFunctionDetailDto {
                                file: file.clone(),
                                name: func.name.clone(),
                                line: func.line,
                                complexity: func.complexity,
                            });
                        }
                    }
                }
                complex_functions_detail.sort_by(|a, b| b.complexity.cmp(&a.complexity));
                complex_functions_detail.truncate(15);
            }

            let anti_patterns_detail: Vec<AntiPatternDetailDto> = anti_patterns.iter()
                .filter(|a| a.kind != AntiPatternKind::GodClass && a.kind != AntiPatternKind::CircularDependency)
                .map(|a| AntiPatternDetailDto {
                    kind: format!("{}", a.kind),
                    description: a.description.clone(),
                    affected: a.affected_modules.clone(),
                    severity: format!("{}", a.severity),
                    suggestion: a.suggestion.clone(),
                })
                .collect();

            let circular_deps_detail: Vec<CircularDepDetailDto> = anti_patterns.iter()
                .filter(|a| a.kind == AntiPatternKind::CircularDependency)
                .map(|a| CircularDepDetailDto {
                    cycle: a.affected_modules.clone(),
                })
                .collect();

            ServiceSnapshotDto {
                name: svc_snap.name.clone(),
                pattern: svc_snap.pattern.clone(),
                total_modules: svc_snap.total_modules,
                total_loc: svc_snap.total_loc,
                anti_pattern_count: svc_snap.anti_pattern_count,
                avg_complexity: svc_snap.avg_complexity,
                max_complexity: svc_snap.max_complexity,
                complex_functions: svc_snap.complex_functions,
                coverage_percent: svc_snap.coverage_percent,
                god_classes: svc_snap.god_classes,
                circular_deps: svc_snap.circular_deps,
                god_classes_detail: Some(god_classes_detail),
                complex_functions_detail: Some(complex_functions_detail),
                anti_patterns_detail: Some(anti_patterns_detail),
                circular_deps_detail: Some(circular_deps_detail),
            }
        }).collect(),
    }
}

fn run_analysis(proj: &void_stack_core::model::Project) -> Result<Vec<(String, analyzer::AnalysisResult)>, String> {
    let mut results = Vec::new();
    for svc in &proj.services {
        let svc_dir = svc.working_dir.as_deref().unwrap_or(&proj.path);
        let clean_svc = strip_win_prefix(svc_dir);
        let svc_path = std::path::Path::new(&clean_svc);
        if let Some(analysis) = analyzer::analyze_project(svc_path) {
            results.push((svc.name.clone(), analysis));
        }
    }
    if results.is_empty() {
        return Err("No se pudo analizar ningún servicio".to_string());
    }
    Ok(results)
}

#[tauri::command]
pub fn analyze_debt(project: String) -> Result<SnapshotDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;
    let results = run_analysis(&proj)?;
    Ok(enriched_dto(&results))
}

#[tauri::command]
pub fn save_debt_snapshot(project: String, label: Option<String>) -> Result<SnapshotDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let clean_path = strip_win_prefix(&proj.path);
    let path = std::path::Path::new(&clean_path);

    let results = run_analysis(&proj)?;
    let snapshot = history::create_snapshot(&results, label);
    history::save_snapshot(path, &snapshot).map_err(|e| e.to_string())?;

    Ok(snap_to_dto(&snapshot))
}

#[tauri::command]
pub fn list_debt_snapshots(project: String) -> Result<Vec<SnapshotDto>, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let clean_path = strip_win_prefix(&proj.path);
    let path = std::path::Path::new(&clean_path);

    let snapshots = history::load_snapshots(path);
    Ok(snapshots.iter().map(snap_to_dto).collect())
}

#[tauri::command]
pub fn compare_debt_snapshots(project: String, index_a: Option<usize>, index_b: Option<usize>) -> Result<DebtComparisonDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let clean_path = strip_win_prefix(&proj.path);
    let path = std::path::Path::new(&clean_path);

    let snapshots = history::load_snapshots(path);
    if snapshots.len() < 2 {
        return Err("Se necesitan al menos 2 snapshots para comparar".to_string());
    }

    let a = index_a.unwrap_or(snapshots.len() - 2);
    let b = index_b.unwrap_or(snapshots.len() - 1);

    let prev = snapshots.get(a).ok_or("Snapshot anterior no encontrado")?;
    let curr = snapshots.get(b).ok_or("Snapshot actual no encontrado")?;

    let comp = history::compare(prev, curr);

    Ok(DebtComparisonDto {
        previous: comp.previous.format("%Y-%m-%d %H:%M:%S").to_string(),
        current: comp.current.format("%Y-%m-%d %H:%M:%S").to_string(),
        overall_trend: format!("{}", comp.overall_trend),
        services: comp.services.iter().map(|s| ServiceComparisonDto {
            name: s.name.clone(),
            loc_delta: s.loc_delta,
            antipattern_delta: s.antipattern_delta,
            complexity_delta: s.complexity_delta,
            coverage_delta: s.coverage_delta,
            god_class_delta: s.god_class_delta,
            circular_dep_delta: s.circular_dep_delta,
            trend: format!("{}", s.trend),
        }).collect(),
    })
}
