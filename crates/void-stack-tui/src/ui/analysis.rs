use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

use crate::app::App;
use crate::i18n::t;

/// Draw the analysis tab showing architecture pattern, layers, anti-patterns, and complexity.
pub fn draw_analysis_tab(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let result = match &app.analysis_result {
        Some(r) => r,
        None => {
            let block = Block::default()
                .title(format!(" {} ", t(l, "analysis.title")))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let hint = if app.analysis_loading {
                t(l, "analysis.running")
            } else {
                t(l, "analysis.run_hint")
            };
            let p = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(p, area);
            return;
        }
    };

    // Check if we have search results or suggestions to show
    #[cfg(feature = "vector")]
    let has_search = app.search_results.is_some() || app.search_active;
    #[cfg(not(feature = "vector"))]
    let has_search = false;
    let has_suggest = app.suggest_output.is_some();
    let has_bottom = has_search || has_suggest;

    // Split into: overview (top) | details (mid) | search/suggest (bottom, if active)
    let chunks = if has_bottom {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),
                Constraint::Min(6),
                Constraint::Length(12),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(6)])
            .split(area)
    };

    // Overview panel
    draw_overview(f, app, result, chunks[0]);

    // Bottom: anti-patterns (left) | complexity (right)
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    draw_anti_patterns(f, app, result, bottom[0]);
    draw_complexity(f, app, result, bottom[1]);

    // Bottom panel: search or suggestions
    if has_bottom && chunks.len() > 2 {
        if has_suggest && !has_search {
            draw_suggest_panel(f, app, chunks[2]);
        } else {
            #[cfg(feature = "vector")]
            if has_search {
                draw_search_panel(f, app, chunks[2]);
            }
        }
    }
}

fn draw_overview(
    f: &mut Frame,
    app: &App,
    result: &void_stack_core::analyzer::AnalysisResult,
    area: Rect,
) {
    let l = app.lang;
    let block = Block::default()
        .title(format!(" {} ", t(l, "analysis.overview")))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();

    let pattern_color = if result.architecture.confidence >= 0.7 {
        Color::Green
    } else if result.architecture.confidence >= 0.4 {
        Color::Yellow
    } else {
        Color::Red
    };

    let project_name = app
        .current_project()
        .map(|p| p.name.as_str())
        .unwrap_or("?");

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Project: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                project_name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {}: ", t(l, "analysis.pattern")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{}", result.architecture.detected_pattern),
                Style::default()
                    .fg(pattern_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " ({:.0}% {})",
                    result.architecture.confidence * 100.0,
                    t(l, "analysis.confidence")
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {}: ", t(l, "analysis.modules")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{}", result.graph.modules.len()),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("  {}: ", t(l, "analysis.loc")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(format!("{}", total_loc), Style::default().fg(Color::White)),
            Span::styled(
                format!("  {}: ", t(l, "analysis.deps")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{}", result.graph.external_deps.len()),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("  {}: ", t(l, "analysis.lang")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{}", result.graph.primary_language),
                Style::default().fg(Color::White),
            ),
        ]),
    ];

    // Layer distribution inline
    let mut layer_parts = vec![Span::styled(
        format!("  {}: ", t(l, "analysis.layers")),
        Style::default().fg(Color::DarkGray),
    )];
    let mut sorted_layers: Vec<_> = result.architecture.layer_distribution.iter().collect();
    sorted_layers.sort_by_key(|x| std::cmp::Reverse(x.1));
    for (i, (layer, count)) in sorted_layers.iter().enumerate() {
        if i > 0 {
            layer_parts.push(Span::styled(", ", Style::default().fg(Color::DarkGray)));
        }
        layer_parts.push(Span::styled(
            format!("{}:{}", layer, count),
            Style::default().fg(Color::White),
        ));
    }
    lines.push(Line::from(layer_parts));

    if let Some(ref cov) = result.coverage {
        let cov_color = if cov.coverage_percent >= 80.0 {
            Color::Green
        } else if cov.coverage_percent >= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {}: ", t(l, "analysis.coverage")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:.1}%", cov.coverage_percent),
                Style::default().fg(cov_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " ({}/{} lines) [{}]",
                    cov.covered_lines, cov.total_lines, cov.tool
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_anti_patterns(
    f: &mut Frame,
    app: &App,
    result: &void_stack_core::analyzer::AnalysisResult,
    area: Rect,
) {
    let l = app.lang;
    let block = Block::default()
        .title(format!(
            " {} ({}) ",
            t(l, "analysis.antipatterns"),
            result.architecture.anti_patterns.len()
        ))
        .borders(Borders::ALL)
        .border_style(
            Style::default().fg(if result.architecture.anti_patterns.is_empty() {
                Color::Green
            } else {
                Color::Yellow
            }),
        );

    if result.architecture.anti_patterns.is_empty() {
        let p = Paragraph::new(Span::styled(
            format!("  {}", t(l, "analysis.no_antipatterns")),
            Style::default().fg(Color::Green),
        ))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from(t(l, "th.severity")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Cell::from(t(l, "th.kind")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Cell::from(t(l, "th.description")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ])
    .height(1);

    let rows: Vec<Row> = result
        .architecture
        .anti_patterns
        .iter()
        .map(|ap| {
            let sev_color = match ap.severity {
                void_stack_core::analyzer::patterns::antipatterns::Severity::High => Color::Red,
                void_stack_core::analyzer::patterns::antipatterns::Severity::Medium => {
                    Color::Yellow
                }
                void_stack_core::analyzer::patterns::antipatterns::Severity::Low => Color::DarkGray,
            };
            Row::new(vec![
                Cell::from(format!("{:?}", ap.severity)).style(Style::default().fg(sev_color)),
                Cell::from(format!("{}", ap.kind)),
                Cell::from(ap.description.clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(20),
            Constraint::Min(30),
        ],
    )
    .header(header)
    .block(block);

    f.render_widget(table, area);
}

fn draw_complexity(
    f: &mut Frame,
    app: &App,
    result: &void_stack_core::analyzer::AnalysisResult,
    area: Rect,
) {
    let l = app.lang;
    let block = Block::default()
        .title(format!(" {} ", t(l, "analysis.complexity")))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let cx = match &result.complexity {
        Some(cx) => cx,
        None => {
            let p = Paragraph::new(Span::styled(
                format!("  {}", t(l, "analysis.no_complexity")),
                Style::default().fg(Color::DarkGray),
            ))
            .block(block);
            f.render_widget(p, area);
            return;
        }
    };

    let mut all_funcs: Vec<_> = cx
        .iter()
        .flat_map(|(path, fc)| fc.functions.iter().map(move |func| (path.as_str(), func)))
        .filter(|(_, func)| func.complexity >= 5)
        .collect();
    all_funcs.sort_by_key(|b| std::cmp::Reverse(b.1.complexity));
    all_funcs.truncate(15);

    let header = Row::new(vec![
        Cell::from(t(l, "th.cc")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Cell::from(t(l, "th.function")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Cell::from(t(l, "th.file")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Cell::from(t(l, "th.cov")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ])
    .height(1);

    let rows: Vec<Row> = all_funcs
        .iter()
        .map(|(path, func)| {
            let cc_color = if func.complexity >= 15 {
                Color::Red
            } else if func.complexity >= 10 {
                Color::Yellow
            } else {
                Color::White
            };
            let short_file = path.rsplit('/').next().unwrap_or(path);
            let cov_str = match func.has_coverage {
                Some(true) => "Y",
                Some(false) => "N",
                None => "-",
            };
            let cov_color = match func.has_coverage {
                Some(true) => Color::Green,
                Some(false) => Color::Red,
                None => Color::DarkGray,
            };
            Row::new(vec![
                Cell::from(format!("{}", func.complexity)).style(Style::default().fg(cc_color)),
                Cell::from(func.name.clone()),
                Cell::from(short_file.to_string()).style(Style::default().fg(Color::DarkGray)),
                Cell::from(cov_str).style(Style::default().fg(cov_color)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Min(20),
            Constraint::Length(20),
            Constraint::Length(4),
        ],
    )
    .header(header)
    .block(block);

    f.render_widget(table, area);
}

#[cfg(feature = "vector")]
fn draw_search_panel(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;

    let idx_badge = if app.indexing {
        "[IDX ...]"
    } else if app.index_exists {
        "[IDX ✓]"
    } else {
        "[SIN IDX]"
    };

    let title = if app.search_active {
        format!(
            " {} {} — /{}█ ",
            t(l, "search.title"),
            idx_badge,
            app.search_input
        )
    } else {
        format!(" {} {} ", t(l, "search.title"), idx_badge)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.search_active {
            Color::Green
        } else {
            Color::DarkGray
        }));

    match &app.search_results {
        Some(results) if !results.is_empty() => {
            let rows: Vec<Row> = results
                .iter()
                .take(5)
                .map(|r| {
                    let score_color = if r.score > 0.8 {
                        Color::Green
                    } else if r.score > 0.6 {
                        Color::Yellow
                    } else {
                        Color::DarkGray
                    };
                    let preview: String =
                        r.chunk.lines().skip(1).take(1).collect::<Vec<_>>().join("");
                    Row::new(vec![
                        Cell::from(format!("{:.2}", r.score))
                            .style(Style::default().fg(score_color)),
                        Cell::from(format!("{}:{}", r.file_path, r.line_start)),
                        Cell::from(preview).style(Style::default().fg(Color::DarkGray)),
                    ])
                })
                .collect();

            let table = Table::new(
                rows,
                [
                    Constraint::Length(5),
                    Constraint::Length(30),
                    Constraint::Min(20),
                ],
            )
            .block(block);
            f.render_widget(table, area);
        }
        _ => {
            let hint = if app.search_active {
                t(l, "search.type_query")
            } else {
                t(l, "search.hint")
            };
            let p = Paragraph::new(Span::styled(
                format!("  {}", hint),
                Style::default().fg(Color::DarkGray),
            ))
            .block(block);
            f.render_widget(p, area);
        }
    }
}

fn draw_suggest_panel(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let title = if app.suggesting {
        format!(" {} ... ", t(l, "suggest.running"))
    } else {
        format!(" {} ", t(l, "help.suggest"))
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.suggesting {
            Color::Yellow
        } else {
            Color::Cyan
        }));

    let text = app.suggest_output.as_deref().unwrap_or("");

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::White));

    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::sample_app;
    use crate::ui::test_utils::render;
    use std::collections::{HashMap, HashSet};
    use void_stack_core::analyzer::AnalysisResult;
    use void_stack_core::analyzer::complexity::{FileComplexity, FunctionComplexity};
    use void_stack_core::analyzer::coverage::CoverageData;
    use void_stack_core::analyzer::graph::{ArchLayer, DependencyGraph, Language, ModuleNode};
    use void_stack_core::analyzer::patterns::antipatterns::{
        AntiPattern, AntiPatternKind, Severity,
    };
    use void_stack_core::analyzer::patterns::{ArchAnalysis, ArchPattern};

    fn module(path: &str, layer: ArchLayer, loc: usize) -> ModuleNode {
        ModuleNode {
            path: path.to_string(),
            language: Language::Rust,
            layer,
            loc,
            class_count: 1,
            function_count: 3,
            is_hub: false,
            has_framework_macros: false,
        }
    }

    /// Build a minimal but populated analysis: 2 modules, 1 anti-pattern,
    /// 1 complex function, plus coverage data.
    fn populated_result() -> AnalysisResult {
        let mut layer_distribution = HashMap::new();
        layer_distribution.insert(ArchLayer::Controller, 1);
        layer_distribution.insert(ArchLayer::Service, 1);

        let mut external_deps = HashSet::new();
        external_deps.insert("serde".to_string());
        external_deps.insert("tokio".to_string());

        let graph = DependencyGraph {
            root_path: "C:\\fixtures\\alpha".to_string(),
            primary_language: Language::Rust,
            modules: vec![
                module("src/controller.rs", ArchLayer::Controller, 100),
                module("src/service.rs", ArchLayer::Service, 50),
            ],
            edges: Vec::new(),
            external_deps,
        };

        let architecture = ArchAnalysis {
            detected_pattern: ArchPattern::Mvc,
            confidence: 0.7,
            layer_distribution,
            anti_patterns: vec![AntiPattern {
                kind: AntiPatternKind::GodClass,
                description: "Big module".to_string(),
                affected_modules: vec!["src/controller.rs".to_string()],
                severity: Severity::High,
                suggestion: "Split it".to_string(),
            }],
        };

        let complexity = vec![(
            "src/service.rs".to_string(),
            FileComplexity {
                functions: vec![FunctionComplexity {
                    name: "process_data".to_string(),
                    line: 10,
                    complexity: 12,
                    loc: 40,
                    has_coverage: Some(true),
                }],
            },
        )];

        AnalysisResult {
            graph,
            architecture,
            coverage: Some(CoverageData {
                tool: "llvm-cov".to_string(),
                total_lines: 200,
                covered_lines: 84,
                coverage_percent: 42.0,
                files: Vec::new(),
            }),
            complexity: Some(complexity),
            explicit_debt: Vec::new(),
        }
    }

    fn analysis_text(app: &App) -> String {
        render(120, 30, |f| {
            let area = f.area();
            draw_analysis_tab(f, app, area);
        })
    }

    #[test]
    fn test_analysis_tab_shows_run_hint_without_result() {
        let app = sample_app();
        let text = analysis_text(&app);
        // Spanish run hint is shown before any analysis has run.
        assert!(text.contains("Analisis"));
    }

    #[test]
    fn test_analysis_tab_shows_loading_message() {
        let mut app = sample_app();
        app.analysis_loading = true;
        let text = analysis_text(&app);
        assert!(text.contains("Analizando"));
    }

    #[test]
    fn test_analysis_overview_renders_summary_numbers() {
        let mut app = sample_app();
        app.analysis_result = Some(populated_result());
        let text = analysis_text(&app);
        // Project name from the selected fixture project.
        assert!(text.contains("alpha"));
        // Detected architecture pattern and confidence.
        assert!(text.contains("MVC"));
        assert!(text.contains("70%"));
        // Coverage branch (Some) is exercised.
        assert!(text.contains("42.0%"));
        assert!(text.contains("llvm-cov"));
    }

    #[test]
    fn test_analysis_renders_anti_pattern_rows() {
        let mut app = sample_app();
        app.analysis_result = Some(populated_result());
        let text = analysis_text(&app);
        // Anti-pattern panel title carries the count.
        assert!(text.contains("Anti-patrones (1)"));
        // Kind and severity cells render.
        assert!(text.contains("God Class"));
        assert!(text.contains("High"));
    }

    #[test]
    fn test_analysis_renders_complexity_rows() {
        let mut app = sample_app();
        app.analysis_result = Some(populated_result());
        let text = analysis_text(&app);
        // Complex function name and its source file render in the table.
        assert!(text.contains("process_data"));
        assert!(text.contains("service.rs"));
    }

    #[test]
    fn test_analysis_no_complexity_data_message() {
        let mut app = sample_app();
        let mut result = populated_result();
        result.complexity = None;
        app.analysis_result = Some(result);
        let text = analysis_text(&app);
        assert!(text.contains("Sin datos de complejidad"));
    }

    #[test]
    fn test_analysis_empty_anti_patterns_shows_clean_message() {
        let mut app = sample_app();
        let mut result = populated_result();
        result.architecture.anti_patterns.clear();
        app.analysis_result = Some(result);
        let text = analysis_text(&app);
        // No-anti-patterns message and zero count in the title.
        assert!(text.contains("Anti-patrones (0)"));
    }

    #[test]
    fn test_analysis_renders_suggest_panel_when_present() {
        let mut app = sample_app();
        app.analysis_result = Some(populated_result());
        app.suggest_output = Some("Refactor the controller".to_string());
        let text = analysis_text(&app);
        assert!(text.contains("Refactor the controller"));
    }
}
