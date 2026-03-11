use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

use crate::app::App;

/// Draw the security audit tab showing risk score and findings.
pub fn draw_security_tab(f: &mut Frame, app: &App, area: Rect) {
    let result = match &app.audit_result {
        Some(r) => r,
        None => {
            let block = Block::default()
                .title(" Security Audit ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let hint = if app.audit_loading {
                "Running audit..."
            } else {
                "Press R to run security audit on the current project"
            };
            let p = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(p, area);
            return;
        }
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(6)])
        .split(area);

    // Risk score overview
    draw_risk_overview(f, result, chunks[0]);

    // Findings table
    draw_findings(f, result, chunks[1]);
}

fn draw_risk_overview(
    f: &mut Frame,
    result: &void_stack_core::audit::AuditResult,
    area: Rect,
) {
    let score_color = if result.summary.risk_score <= 20.0 {
        Color::Green
    } else if result.summary.risk_score <= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    };

    let block = Block::default()
        .title(" Risk Overview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(score_color));

    let critical = result.findings.iter()
        .filter(|f| matches!(f.severity, void_stack_core::audit::findings::Severity::Critical))
        .count();
    let high = result.findings.iter()
        .filter(|f| matches!(f.severity, void_stack_core::audit::findings::Severity::High))
        .count();
    let medium = result.findings.iter()
        .filter(|f| matches!(f.severity, void_stack_core::audit::findings::Severity::Medium))
        .count();
    let low = result.findings.iter()
        .filter(|f| matches!(f.severity, void_stack_core::audit::findings::Severity::Low))
        .count();

    let lines = vec![
        Line::from(vec![
            Span::styled("  Risk Score: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.0}/100", result.summary.risk_score),
                Style::default().fg(score_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  Total findings: {}", result.findings.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(format!("{} critical", critical), Style::default().fg(Color::Red)),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} high", high), Style::default().fg(Color::LightRed)),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} medium", medium), Style::default().fg(Color::Yellow)),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} low", low), Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_findings(
    f: &mut Frame,
    result: &void_stack_core::audit::AuditResult,
    area: Rect,
) {
    let block = Block::default()
        .title(format!(" Findings ({}) ", result.findings.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    if result.findings.is_empty() {
        let p = Paragraph::new(Span::styled(
            "  No security issues found!",
            Style::default().fg(Color::Green),
        ))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Sev").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Category").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("File").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Line").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Description").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
    ])
    .height(1);

    let rows: Vec<Row> = result
        .findings
        .iter()
        .map(|finding| {
            let sev_color = match finding.severity {
                void_stack_core::audit::findings::Severity::Critical => Color::Red,
                void_stack_core::audit::findings::Severity::High => Color::LightRed,
                void_stack_core::audit::findings::Severity::Medium => Color::Yellow,
                void_stack_core::audit::findings::Severity::Low => Color::DarkGray,
                void_stack_core::audit::findings::Severity::Info => Color::Blue,
            };
            let short_file = finding.file_path.as_deref()
                .map(|p| p.rsplit('/').next().unwrap_or(p))
                .unwrap_or("-");
            let line_str = finding.line_number
                .map(|l| l.to_string())
                .unwrap_or_else(|| "-".to_string());

            Row::new(vec![
                Cell::from(format!("{}", finding.severity)).style(Style::default().fg(sev_color)),
                Cell::from(format!("{}", finding.category)),
                Cell::from(short_file.to_string()).style(Style::default().fg(Color::DarkGray)),
                Cell::from(line_str),
                Cell::from(finding.description.clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(18),
            Constraint::Length(18),
            Constraint::Length(5),
            Constraint::Min(30),
        ],
    )
    .header(header)
    .block(block);

    f.render_widget(table, area);
}
