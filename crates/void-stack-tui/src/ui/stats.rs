use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::App;
use crate::i18n::t;

pub fn draw_stats_tab(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let report = match &app.stats_report {
        Some(r) => r,
        None => {
            let block = Block::default()
                .title(format!(" {} ", t(l, "stats.title")))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let hint = if app.stats_loading {
                t(l, "stats.running")
            } else {
                t(l, "stats.run_hint")
            };
            let p = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(p, area);
            return;
        }
    };

    let block = Block::default()
        .title(format!(
            " {} — {} {} | {:.0}% {} | {} {} ",
            t(l, "stats.title"),
            report.total_operations,
            t(l, "stats.ops"),
            report.avg_savings_pct,
            t(l, "stats.avg"),
            report.total_lines_saved,
            t(l, "stats.lines_saved"),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Build rows: projects first, then operations
    let mut rows: Vec<Row> = Vec::new();

    // Section: by project
    rows.push(Row::new(vec![
        Cell::from(Span::styled(
            t(l, "stats.by_project"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
    ]));

    for p in &report.by_project {
        rows.push(Row::new(vec![
            Cell::from(format!("  {}", p.project)),
            Cell::from(format!("{:.0}%", p.avg_savings_pct)),
            Cell::from(format!("{}", p.operations)),
            Cell::from(format!("{}", p.lines_saved)),
        ]));
    }

    // Separator
    rows.push(Row::new(vec![Cell::from(""); 4]));

    // Section: by operation
    rows.push(Row::new(vec![
        Cell::from(Span::styled(
            t(l, "stats.by_operation"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
    ]));

    for o in &report.by_operation {
        rows.push(Row::new(vec![
            Cell::from(format!("  {}", o.operation)),
            Cell::from(format!("{:.0}%", o.avg_savings_pct)),
            Cell::from(format!("{}", o.operations)),
            Cell::from(format!("{}", o.lines_saved)),
        ]));
    }

    let header = Row::new(vec![
        Cell::from(Span::styled(
            t(l, "th.name"),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            t(l, "stats.savings"),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            t(l, "stats.ops"),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            t(l, "stats.lines_saved"),
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ])
    .style(Style::default().fg(Color::Cyan));

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(30),
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
    use void_stack_core::stats::{OperationStats, ProjectStats, StatsReport};

    fn stats_text(app: &App) -> String {
        render(100, 24, |f| {
            let area = f.area();
            draw_stats_tab(f, app, area);
        })
    }

    #[test]
    fn test_stats_tab_shows_run_hint_without_report() {
        let app = sample_app();
        let text = stats_text(&app);
        assert!(text.contains("Token Savings"));
        assert!(text.contains("Presiona R para cargar"));
    }

    #[test]
    fn test_stats_tab_shows_loading_message() {
        let mut app = sample_app();
        app.stats_loading = true;
        let text = stats_text(&app);
        assert!(text.contains("Cargando estadisticas..."));
    }

    #[test]
    fn test_stats_tab_renders_report_sections() {
        let mut app = sample_app();
        app.stats_report = Some(StatsReport {
            total_operations: 42,
            avg_savings_pct: 80.0,
            total_lines_saved: 1234,
            by_project: vec![ProjectStats {
                project: "alpha".to_string(),
                avg_savings_pct: 75.0,
                operations: 10,
                lines_saved: 500,
            }],
            by_operation: vec![OperationStats {
                operation: "git".to_string(),
                avg_savings_pct: 85.0,
                operations: 32,
                lines_saved: 734,
            }],
            period_days: 30,
        });

        let text = stats_text(&app);
        assert!(text.contains("42 ops"));
        assert!(text.contains("Por proyecto"));
        assert!(text.contains("Por operacion"));
        assert!(text.contains("alpha"));
        assert!(text.contains("git"));
        assert!(text.contains("75%"));
    }
}
