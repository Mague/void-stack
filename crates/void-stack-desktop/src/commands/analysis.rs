use serde::Serialize;

use void_stack_core::analyzer;
use void_stack_core::global_config::load_global_config;
use void_stack_core::runner::local::strip_win_prefix;

use crate::state::AppState;

#[derive(Serialize)]
pub struct AnalysisResultDto {
    pub pattern: String,
    pub confidence: f32,
    pub layers: Vec<LayerDto>,
    pub anti_patterns: Vec<AntiPatternDto>,
    pub top_complex: Vec<ComplexFunctionDto>,
    pub coverage: Option<CoverageDto>,
    pub coverage_hint: Option<String>,
    pub module_count: usize,
    pub total_loc: usize,
    pub markdown: String,
    pub best_practices: Option<BestPracticesResultDto>,
    pub explicit_debt: Vec<ExplicitDebtDto>,
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
    pub has_coverage: Option<bool>,
}

#[derive(Serialize)]
pub struct ExplicitDebtDto {
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub text: String,
    pub language: String,
}

#[derive(Serialize)]
pub struct CoverageDto {
    pub tool: String,
    pub percent: f32,
    pub covered: usize,
    pub total: usize,
}

#[derive(Serialize)]
pub struct BestPracticesResultDto {
    pub overall_score: f32,
    pub tools_used: Vec<String>,
    pub tool_scores: Vec<ToolScoreDto>,
    pub findings: Vec<BpFindingDto>,
}

#[derive(Serialize)]
pub struct ToolScoreDto {
    pub tool: String,
    pub score: f32,
    pub finding_count: usize,
    pub native_score: Option<f32>,
}

#[derive(Serialize)]
pub struct BpFindingDto {
    pub rule_id: String,
    pub tool: String,
    pub category: String,
    pub severity: String,
    pub file: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
    pub fix_hint: Option<String>,
}

#[tauri::command]
pub async fn analyze_project_cmd(
    project: String,
    best_practices: Option<bool>,
    bp_only: Option<bool>,
    service: Option<String>,
) -> Result<AnalysisResultDto, String> {
    tokio::task::spawn_blocking(move || {
        analyze_project_sync(project, best_practices, bp_only, service)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

fn analyze_project_sync(
    project: String,
    best_practices: Option<bool>,
    bp_only: Option<bool>,
    service: Option<String>,
) -> Result<AnalysisResultDto, String> {
    let config = load_global_config().map_err(|e| e.to_string())?;
    let proj = AppState::find_project(&config, &project)?;

    let clean_path = strip_win_prefix(&proj.path);
    // If a specific service is requested, analyze its directory
    let analysis_path = if let Some(ref svc_name) = service {
        let svc = proj
            .services
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(svc_name))
            .ok_or_else(|| format!("Servicio '{}' no encontrado", svc_name))?;
        let svc_dir = svc.working_dir.as_deref().unwrap_or(&proj.path);
        strip_win_prefix(svc_dir)
    } else {
        clean_path.clone()
    };
    let path = std::path::Path::new(&analysis_path);

    // bp-only: return only best practices without architecture analysis
    if bp_only.unwrap_or(false) {
        let bp_result = analyzer::best_practices::analyze_best_practices(path);
        return Ok(AnalysisResultDto {
            pattern: String::new(),
            confidence: 0.0,
            layers: vec![],
            anti_patterns: vec![],
            top_complex: vec![],
            coverage: None,
            coverage_hint: None,
            module_count: 0,
            total_loc: 0,
            markdown: String::new(),
            explicit_debt: vec![],
            best_practices: Some(BestPracticesResultDto {
                overall_score: bp_result.overall_score,
                tools_used: bp_result.tools_used.clone(),
                tool_scores: bp_result
                    .tool_scores
                    .iter()
                    .map(|ts| ToolScoreDto {
                        tool: ts.tool.clone(),
                        score: ts.score,
                        finding_count: ts.finding_count,
                        native_score: ts.native_score,
                    })
                    .collect(),
                findings: bp_result
                    .findings
                    .iter()
                    .map(|f| BpFindingDto {
                        rule_id: f.rule_id.clone(),
                        tool: f.tool.clone(),
                        category: format!("{}", f.category),
                        severity: format!("{}", f.severity),
                        file: f.file.clone(),
                        line: f.line,
                        col: f.col,
                        message: f.message.clone(),
                        fix_hint: f.fix_hint.clone(),
                    })
                    .collect(),
            }),
        });
    }

    let result = analyzer::analyze_project(path).ok_or_else(|| {
        "No se pudo analizar el proyecto (sin archivos fuente detectados)".to_string()
    })?;

    let markdown = analyzer::generate_docs(&result, &proj.name);

    let mut layers: Vec<LayerDto> = result
        .architecture
        .layer_distribution
        .iter()
        .map(|(layer, count)| LayerDto {
            name: format!("{}", layer),
            count: *count,
        })
        .collect();
    layers.sort_by(|a, b| b.count.cmp(&a.count));

    let anti_patterns: Vec<AntiPatternDto> = result
        .architecture
        .anti_patterns
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
                        has_coverage: func.has_coverage,
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

    // Generate coverage hint when no coverage data found
    let coverage_hint = if coverage.is_none() {
        use std::collections::HashSet;
        use void_stack_core::analyzer::graph::Language;
        let languages: HashSet<_> = result.graph.modules.iter().map(|m| m.language).collect();
        let mut hints = Vec::new();
        if languages.contains(&Language::Rust) {
            hints.push("Rust: cargo install cargo-tarpaulin && cargo tarpaulin --out xml");
        }
        if languages.contains(&Language::Python) {
            hints.push("Python: pip install pytest-cov && pytest --cov --cov-report=xml");
        }
        if languages.contains(&Language::JavaScript) || languages.contains(&Language::TypeScript) {
            hints.push("JS/TS: npx c8 --reporter=lcov npm test");
        }
        if languages.contains(&Language::Go) {
            hints.push("Go: go test -coverprofile=coverage.out ./...");
        }
        if languages.contains(&Language::Dart) {
            hints.push("Flutter: flutter test --coverage");
        }
        if hints.is_empty() {
            None
        } else {
            Some(hints.join("\n"))
        }
    } else {
        None
    };

    let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();

    // Best practices (if requested)
    let bp = if best_practices.unwrap_or(false) {
        let bp_result = analyzer::best_practices::analyze_best_practices(path);
        Some(BestPracticesResultDto {
            overall_score: bp_result.overall_score,
            tools_used: bp_result.tools_used.clone(),
            tool_scores: bp_result
                .tool_scores
                .iter()
                .map(|ts| ToolScoreDto {
                    tool: ts.tool.clone(),
                    score: ts.score,
                    finding_count: ts.finding_count,
                    native_score: ts.native_score,
                })
                .collect(),
            findings: bp_result
                .findings
                .iter()
                .map(|f| BpFindingDto {
                    rule_id: f.rule_id.clone(),
                    tool: f.tool.clone(),
                    category: format!("{}", f.category),
                    severity: format!("{}", f.severity),
                    file: f.file.clone(),
                    line: f.line,
                    col: f.col,
                    message: f.message.clone(),
                    fix_hint: f.fix_hint.clone(),
                })
                .collect(),
        })
    } else {
        None
    };

    let explicit_debt: Vec<ExplicitDebtDto> = result
        .explicit_debt
        .iter()
        .map(|d| ExplicitDebtDto {
            file: d.file.clone(),
            line: d.line,
            kind: d.kind.clone(),
            text: d.text.clone(),
            language: d.language.clone(),
        })
        .collect();

    Ok(AnalysisResultDto {
        pattern: format!("{}", result.architecture.detected_pattern),
        confidence: result.architecture.confidence,
        layers,
        anti_patterns,
        top_complex,
        coverage,
        coverage_hint,
        module_count: result.graph.modules.len(),
        total_loc,
        markdown,
        explicit_debt,
        best_practices: bp,
    })
}
