use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

use crate::app::App;
use crate::i18n::t;

/// Draw the security audit tab showing risk score and findings.
pub fn draw_security_tab(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let result = match &app.audit_result {
        Some(r) => r,
        None => {
            let block = Block::default()
                .title(format!(" {} ", t(l, "security.title")))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let hint = if app.audit_loading {
                t(l, "security.running")
            } else {
                t(l, "security.run_hint")
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
    draw_risk_overview(f, app, result, chunks[0]);

    // Findings table
    draw_findings(f, app, result, chunks[1]);
}

fn draw_risk_overview(
    f: &mut Frame,
    app: &App,
    result: &void_stack_core::audit::AuditResult,
    area: Rect,
) {
    let l = app.lang;
    let score_color = if result.summary.risk_score <= 20.0 {
        Color::Green
    } else if result.summary.risk_score <= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    };

    let block = Block::default()
        .title(format!(" {} ", t(l, "security.risk")))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(score_color));

    let critical = result
        .findings
        .iter()
        .filter(|f| {
            matches!(
                f.severity,
                void_stack_core::audit::findings::Severity::Critical
            )
        })
        .count();
    let high = result
        .findings
        .iter()
        .filter(|f| matches!(f.severity, void_stack_core::audit::findings::Severity::High))
        .count();
    let medium = result
        .findings
        .iter()
        .filter(|f| {
            matches!(
                f.severity,
                void_stack_core::audit::findings::Severity::Medium
            )
        })
        .count();
    let low = result
        .findings
        .iter()
        .filter(|f| matches!(f.severity, void_stack_core::audit::findings::Severity::Low))
        .count();

    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("  {}: ", t(l, "security.score")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:.0}/100", result.summary.risk_score),
                Style::default()
                    .fg(score_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}: {}", t(l, "security.total"), result.findings.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{} {}", critical, t(l, "security.critical")),
                Style::default().fg(Color::Red),
            ),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} {}", high, t(l, "security.high")),
                Style::default().fg(Color::LightRed),
            ),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} {}", medium, t(l, "security.medium")),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} {}", low, t(l, "security.low")),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_findings(
    f: &mut Frame,
    app: &App,
    result: &void_stack_core::audit::AuditResult,
    area: Rect,
) {
    let l = app.lang;
    let block = Block::default()
        .title(format!(
            " {} ({}) ",
            t(l, "security.findings"),
            result.findings.len()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    if result.findings.is_empty() {
        let p = Paragraph::new(Span::styled(
            format!("  {}", t(l, "security.no_findings")),
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
        Cell::from(t(l, "th.category")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Cell::from(t(l, "th.file")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Cell::from(t(l, "th.line")).style(
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
            let short_file = finding
                .file_path
                .as_deref()
                .map(|p| p.rsplit('/').next().unwrap_or(p))
                .unwrap_or("-");
            let line_str = finding
                .line_number
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::sample_app;
    use crate::ui::test_utils::render;
    use void_stack_core::audit::AuditResult;
    use void_stack_core::audit::findings::{FindingCategory, SecurityFinding, Severity};

    fn finding(
        id: &str,
        severity: Severity,
        category: FindingCategory,
        file: &str,
        line: u32,
        desc: &str,
    ) -> SecurityFinding {
        SecurityFinding::new(
            id.to_string(),
            severity,
            category,
            "Title".to_string(),
            desc.to_string(),
            Some(file.to_string()),
            Some(line),
            "Fix it".to_string(),
        )
    }

    /// Audit with findings across three severities.
    fn populated_audit() -> AuditResult {
        let mut result = AuditResult::new("alpha", "C:\\fixtures\\alpha");
        result.add_finding(finding(
            "F1",
            Severity::Critical,
            FindingCategory::HardcodedSecret,
            "src/secrets.rs",
            5,
            "leaked key",
        ));
        result.add_finding(finding(
            "F2",
            Severity::High,
            FindingCategory::SqlInjection,
            "src/db.rs",
            12,
            "raw sql",
        ));
        result.add_finding(finding(
            "F3",
            Severity::Medium,
            FindingCategory::DebugEnabled,
            "src/config.rs",
            3,
            "debug on",
        ));
        result.summary.risk_score = 65.0;
        result
    }

    fn security_text(app: &App) -> String {
        render(120, 30, |f| {
            let area = f.area();
            draw_security_tab(f, app, area);
        })
    }

    #[test]
    fn test_security_tab_shows_run_hint_without_result() {
        let app = sample_app();
        let text = security_text(&app);
        assert!(text.contains("Auditoria de Seguridad"));
    }

    #[test]
    fn test_security_tab_shows_loading_message() {
        let mut app = sample_app();
        app.audit_loading = true;
        let text = security_text(&app);
        assert!(text.contains("Ejecutando auditoria"));
    }

    #[test]
    fn test_security_risk_overview_renders_score_and_counts() {
        let mut app = sample_app();
        app.audit_result = Some(populated_audit());
        let text = security_text(&app);
        // Risk score rendered as N/100.
        assert!(text.contains("65/100"));
        // Severity count labels with their tallies.
        assert!(text.contains("1 criticos"));
        assert!(text.contains("1 altos"));
        assert!(text.contains("1 medios"));
        assert!(text.contains("0 bajos"));
    }

    #[test]
    fn test_security_findings_table_renders_severity_and_category() {
        let mut app = sample_app();
        app.audit_result = Some(populated_audit());
        let text = security_text(&app);
        // Findings panel title carries the count.
        assert!(text.contains("Hallazgos (3)"));
        // Severity labels (Display is lowercase). The severity column is 6
        // chars wide so "critical" is clipped to "critic".
        assert!(text.contains("critic"));
        assert!(text.contains("high"));
        assert!(text.contains("medium"));
        // Category cells render.
        assert!(text.contains("Hardcoded secret"));
        assert!(text.contains("SQL injection"));
    }

    #[test]
    fn test_security_no_findings_shows_clean_message() {
        let mut app = sample_app();
        app.audit_result = Some(AuditResult::new("alpha", "C:\\fixtures\\alpha"));
        let text = security_text(&app);
        assert!(text.contains("Hallazgos (0)"));
    }
}
