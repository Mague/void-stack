use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::App;

/// Draw the space scanner tab showing project and global disk usage.
pub fn draw_space_tab(f: &mut Frame, app: &App, area: Rect) {
    let entries = match &app.space_entries {
        Some(e) => e,
        None => {
            let block = Block::default()
                .title(" Disk Space ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let hint = if app.space_loading {
                "Scanning disk space..."
            } else {
                "Press R to scan project + global disk usage"
            };
            let p = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(p, area);
            return;
        }
    };

    if entries.is_empty() {
        let block = Block::default()
            .title(" Disk Space ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));
        let p = Paragraph::new(Span::styled(
            "  No cleanable directories found",
            Style::default().fg(Color::Green),
        ))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let total_bytes: u64 = entries.iter().map(|e| e.size_bytes).sum();
    let total_human = format_size(total_bytes);

    let block = Block::default()
        .title(format!(
            " Disk Space — {} entries, {} total ",
            entries.len(),
            total_human,
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let header = Row::new(vec![
        Cell::from("Category").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Name").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Size").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Cell::from("Path").style(Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
    ])
    .height(1);

    let rows: Vec<Row> = entries
        .iter()
        .map(|entry| {
            let size_color = if entry.size_bytes >= 500_000_000 {
                Color::Red
            } else if entry.size_bytes >= 100_000_000 {
                Color::Yellow
            } else {
                Color::White
            };
            let short_path = if entry.path.len() > 40 {
                format!("...{}", &entry.path[entry.path.len() - 37..])
            } else {
                entry.path.clone()
            };
            Row::new(vec![
                Cell::from(format!("{:?}", entry.category)).style(Style::default().fg(Color::DarkGray)),
                Cell::from(entry.name.clone()).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Cell::from(entry.size_human.clone()).style(Style::default().fg(size_color)),
                Cell::from(short_path).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(20),
            Constraint::Length(10),
            Constraint::Min(30),
        ],
    )
    .header(header)
    .block(block);

    f.render_widget(table, area);
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
