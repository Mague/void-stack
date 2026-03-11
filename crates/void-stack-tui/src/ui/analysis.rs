use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

use crate::app::App;

/// Draw the analysis tab showing architecture pattern, layers, anti-patterns, and complexity.
pub fn draw_analysis_tab(f: &mut Frame, app: &App, area: Rect) {
    let result = match &app.analysis_result {
        Some(r) => r,
        None => {
            let block = Block::default()
                .title(" Analysis ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let hint = if app.analysis_loading {
                "Analyzing..."
            } else {
                "Press R to run analysis on the current project"
            };
            let p = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(p, area);
            return;
        }
    };

    // Split into: overview (top) | details (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(6)])
        .split(area);

    // Overview panel
    draw_overview(f, app, result, chunks[0]);

    // Bottom: anti-patterns (left) | complexity (right)
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    draw_anti_patterns(f, result, bottom[0]);
    draw_complexity(f, result, bottom[1]);
}

fn draw_overview(
    f: &mut Frame,
    app: &App,
    result: &void_stack_core::analyzer::AnalysisResult,
    area: Rect,
) {
    let block = Block::default()
        .title(" Architecture Overview ")
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

    let project_name = app.current_project().map(|p| p.name.as_str()).unwrap_or("?");

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Project: ", Style::default().fg(Color::DarkGray)),
            Span::styled(project_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Pattern: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", result.architecture.detected_pattern),
                Style::default().fg(pattern_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({:.0}% confidence)", result.architecture.confidence * 100.0),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Modules: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", result.graph.modules.len()), Style::default().fg(Color::White)),
            Span::styled("  LOC: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", total_loc), Style::default().fg(Color::White)),
            Span::styled("  Deps: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", result.graph.external_deps.len()), Style::default().fg(Color::White)),
            Span::styled("  Lang: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", result.graph.primary_language), Style::default().fg(Color::White)),
        ]),
    ];

    // Layer distribution inline
    let mut layer_parts = vec![Span::styled("  Layers: ", Style::default().fg(Color::DarkGray))];
    let mut sorted_layers: Vec<_> = result.architecture.layer_distribution.iter().collect();
    sorted_layers.sort_by(|a, b| b.1.cmp(a.1));
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

    // Coverage if available
    if let Some(ref cov) = result.coverage {
        let cov_color = if cov.coverage_percent >= 80.0 {
            Color::Green
        } else if cov.coverage_percent >= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        lines.push(Line::from(vec![
            Span::styled("  Coverage: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.1}%", cov.coverage_percent),
                Style::default().fg(cov_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({}/{} lines) [{}]", cov.covered_lines, cov.total_lines, cov.tool),
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
    result: &void_stack_core::analyzer::AnalysisResult,
    area: Rect,
) {
    let block = Block::default()
        .title(format!(" Anti-patterns ({}) ", result.architecture.anti_patterns.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if result.architecture.anti_patterns.is_empty() {
            Color::Green
        } else {
            Color::Yellow
        }));

    if result.architecture.anti_patterns.is_empty() {
        let p = Paragraph::new(Span::styled(
            "  No anti-patterns detected",
            Style::default().fg(Color::Green),
        ))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Severity").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Kind").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Description").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
    ])
    .height(1);

    let rows: Vec<Row> = result
        .architecture
        .anti_patterns
        .iter()
        .map(|ap| {
            let sev_color = match ap.severity {
                void_stack_core::analyzer::patterns::antipatterns::Severity::High => Color::Red,
                void_stack_core::analyzer::patterns::antipatterns::Severity::Medium => Color::Yellow,
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
    result: &void_stack_core::analyzer::AnalysisResult,
    area: Rect,
) {
    let block = Block::default()
        .title(" Top Complex Functions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let cx = match &result.complexity {
        Some(cx) => cx,
        None => {
            let p = Paragraph::new(Span::styled(
                "  No complexity data",
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
    all_funcs.sort_by(|a, b| b.1.complexity.cmp(&a.1.complexity));
    all_funcs.truncate(15);

    let header = Row::new(vec![
        Cell::from("CC").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Function").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("File").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Cov").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
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
