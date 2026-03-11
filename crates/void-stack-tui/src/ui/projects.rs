use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};

use void_stack_core::model::ServiceStatus;

use crate::app::App;

/// Draw the project list panel (used by all tabs).
pub fn draw_projects_panel(f: &mut Frame, app: &App, area: Rect, highlight: bool) {
    let border_color = if highlight { Color::Cyan } else { Color::DarkGray };

    let block = Block::default()
        .title(" Projects ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let items: Vec<ListItem> = app
        .projects
        .iter()
        .enumerate()
        .map(|(i, project)| {
            let running = project
                .states
                .iter()
                .filter(|s| s.status == ServiceStatus::Running)
                .count();
            let total = project.service_names.len();

            let indicator = if running > 0 { "●" } else { "○" };
            let indicator_color = if running > 0 { Color::Green } else { Color::DarkGray };

            let style = if i == app.selected_project {
                Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", indicator), Style::default().fg(indicator_color)),
                Span::styled(format!("{} ", project.name), style),
                Span::styled(
                    format!("[{}/{}]", running, total),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
            .style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}
