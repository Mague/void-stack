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
    let mut summary_parts: Vec<String> = by_kind.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
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
        Cell::from(t(l, "th.kind")).style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from(t(l, "th.file")).style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from(t(l, "th.line")).style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from(t(l, "th.text")).style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
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
                Cell::from(item.kind.clone()).style(Style::default().fg(kind_color).add_modifier(Modifier::BOLD)),
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
