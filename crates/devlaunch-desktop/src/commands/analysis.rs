use serde::Serialize;

use devlaunch_core::analyzer;
use devlaunch_core::global_config::load_global_config;
use devlaunch_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct AnalysisResultDto {
    pub pattern: String,
    pub confidence: f32,
    pub layers: Vec<LayerDto>,
    pub anti_patterns: Vec<AntiPatternDto>,
    pub top_complex: Vec<ComplexFunctionDto>,
    pub coverage: Option<CoverageDto>,
    pub module_count: usize,
    pub total_loc: usize,
    pub markdown: String,
}

#[derive(Serialize)]
pub struct LayerDto {
    pub name: String,
    pub count: usize,
}

#[derive(Serialize)]
pub struct AntiPatternDto {
    pub kind: String,
    pub description: String,
    pub affected: Vec<String>,
    pub severity: String,
    pub suggestion: String,
}

#[derive(Serialize)]
pub struct ComplexFunctionDto {
    pub file: String,
    pub name: String,
    pub line: usize,
    pub complexity: usize,
}

#[derive(Serialize)]
pub struct CoverageDto {
    pub tool: String,
    pub percent: f32,
    pub covered: usize,
    pub total: usize,
}

#[tauri::command]
pub fn analyze_project_cmd(project: String) -> Result<AnalysisResultDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let clean_path = strip_win_prefix(&proj.path);
    let path = std::path::Path::new(&clean_path);
    let result = analyzer::analyze_project(path)
        .ok_or_else(|| "No se pudo analizar el proyecto (sin archivos fuente detectados)".to_string())?;

    let markdown = analyzer::generate_docs(&result, &proj.name);

    let mut layers: Vec<LayerDto> = result.architecture.layer_distribution
        .iter()
        .map(|(layer, count)| LayerDto {
            name: format!("{}", layer),
            count: *count,
        })
        .collect();
    layers.sort_by(|a, b| b.count.cmp(&a.count));

    let anti_patterns: Vec<AntiPatternDto> = result.architecture.anti_patterns
        .iter()
        .map(|ap| AntiPatternDto {
            kind: format!("{}", ap.kind),
            description: ap.description.clone(),
            affected: ap.affected_modules.clone(),
            severity: format!("{}", ap.severity),
            suggestion: ap.suggestion.clone(),
        })
        .collect();

    let mut top_complex = Vec::new();
    if let Some(ref complexity) = result.complexity {
        for (file, fc) in complexity {
            for func in &fc.functions {
                if func.complexity >= 5 {
                    top_complex.push(ComplexFunctionDto {
                        file: file.clone(),
                        name: func.name.clone(),
                        line: func.line,
                        complexity: func.complexity,
                    });
                }
            }
        }
        top_complex.sort_by(|a, b| b.complexity.cmp(&a.complexity));
        top_complex.truncate(20);
    }

    let coverage = result.coverage.as_ref().map(|c| CoverageDto {
        tool: c.tool.clone(),
        percent: c.coverage_percent,
        covered: c.covered_lines,
        total: c.total_lines,
    });

    let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();

    Ok(AnalysisResultDto {
        pattern: format!("{}", result.architecture.detected_pattern),
        confidence: result.architecture.confidence,
        layers,
        anti_patterns,
        top_complex,
        coverage,
        module_count: result.graph.modules.len(),
        total_loc,
        markdown,
    })
}
