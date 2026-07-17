use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::App;
use crate::i18n::t;

/// Draw the explicit debt tab showing TODO/FIXME/HACK markers found in source code.
pub fn draw_debt_tab(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let items = match &app.debt_items {
        Some(items) => items,
        None => {
            let block = Block::default()
                .title(format!(" {} ", t(l, "debt.title")))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let hint = if app.debt_loading {
                t(l, "debt.running")
            } else {
                t(l, "debt.run_hint")
            };
            let p = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(p, area);
            return;
        }
    };

    if items.is_empty() {
        let block = Block::default()
            .title(format!(" {} ", t(l, "debt.title")))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));
        let p = Paragraph::new(Span::styled(
            format!("  {}", t(l, "debt.no_markers")),
            Style::default().fg(Color::Green),
        ))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    // Summary line
    let mut by_kind: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for item in items {
        *by_kind.entry(&item.kind).or_insert(0) += 1;
    }
    let mut summary_parts: Vec<String> = by_kind
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect();
    summary_parts.sort();

    let title = format!(
        " {} — {} {} ({}) ",
        t(l, "debt.title"),
        items.len(),
        t(l, "debt.markers"),
        summary_parts.join(", "),
    );

    let block = Block::default()
        .title(Line::from(Span::styled(
            title,
            Style::default().fg(Color::Yellow),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let header = Row::new(vec![
        Cell::from(t(l, "th.kind")).style(
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
        Cell::from(t(l, "th.text")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ])
    .height(1);

    let rows: Vec<Row> = items
        .iter()
        .map(|item| {
            let kind_color = match item.kind.as_str() {
                "FIXME" | "BUG" => Color::Red,
                "HACK" | "TEMP" | "WORKAROUND" => Color::Yellow,
                "TODO" => Color::Cyan,
                "OPTIMIZE" => Color::Green,
                _ => Color::White,
            };
            let short_file = item.file.rsplit('/').next().unwrap_or(&item.file);
            let text = if item.text.len() > 60 {
                format!("{}...", &item.text[..57])
            } else {
                item.text.clone()
            };
            Row::new(vec![
                Cell::from(item.kind.clone())
                    .style(Style::default().fg(kind_color).add_modifier(Modifier::BOLD)),
                Cell::from(short_file.to_string()).style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!("{}", item.line)),
                Cell::from(text),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(22),
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
    use void_stack_core::analyzer::explicit_debt::ExplicitDebtItem;

    fn item(kind: &str, file: &str, line: usize, text: &str) -> ExplicitDebtItem {
        ExplicitDebtItem {
            file: file.to_string(),
            line,
            kind: kind.to_string(),
            text: text.to_string(),
            language: "Rust".to_string(),
        }
    }

    fn populated_items() -> Vec<ExplicitDebtItem> {
        vec![
            item("TODO", "src/main.rs", 10, "wire up config"),
            item("FIXME", "src/api.rs", 42, "handle timeout"),
        ]
    }

    fn debt_text(app: &App) -> String {
        render(120, 30, |f| {
            let area = f.area();
            draw_debt_tab(f, app, area);
        })
    }

    #[test]
    fn test_debt_tab_shows_run_hint_without_items() {
        let app = sample_app();
        let text = debt_text(&app);
        assert!(text.contains("Deuda Tecnica"));
    }

    #[test]
    fn test_debt_tab_shows_loading_message() {
        let mut app = sample_app();
        app.debt_loading = true;
        let text = debt_text(&app);
        assert!(text.contains("Escaneando"));
    }

    #[test]
    fn test_debt_empty_shows_clean_message() {
        let mut app = sample_app();
        app.debt_items = Some(Vec::new());
        let text = debt_text(&app);
        assert!(text.contains("Sin marcadores de deuda"));
    }

    #[test]
    fn test_debt_renders_marker_rows_and_summary() {
        let mut app = sample_app();
        app.debt_items = Some(populated_items());
        let text = debt_text(&app);
        // Title carries the total count and per-kind summary.
        assert!(text.contains("2 marcadores"));
        assert!(text.contains("FIXME: 1"));
        assert!(text.contains("TODO: 1"));
        // Kind, file and marker text render in the table.
        assert!(text.contains("TODO"));
        assert!(text.contains("FIXME"));
        assert!(text.contains("main.rs"));
        assert!(text.contains("api.rs"));
        assert!(text.contains("wire up config"));
        assert!(text.contains("handle timeout"));
    }
}
