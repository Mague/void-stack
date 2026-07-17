use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};

use void_stack_core::model::ServiceStatus;

use crate::app::App;
use crate::i18n::t;

/// Draw the project list panel (used by all tabs).
pub fn draw_projects_panel(f: &mut Frame, app: &App, area: Rect, highlight: bool) {
    let border_color = if highlight {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(format!(" {} ", t(app.lang, "panel.projects")))
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
            let indicator_color = if running > 0 {
                Color::Green
            } else {
                Color::DarkGray
            };

            let style = if i == app.selected_project {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", indicator),
                    Style::default().fg(indicator_color),
                ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::sample_app;
    use crate::ui::test_utils::render;

    #[test]
    fn test_projects_panel_lists_projects_with_running_counts() {
        let app = sample_app();
        let text = render(40, 10, |f| {
            let area = f.area();
            draw_projects_panel(f, &app, area, true);
        });

        assert!(text.contains("Proyectos"));
        assert!(text.contains("alpha"));
        assert!(text.contains("beta"));
        assert!(text.contains("[1/2]")); // alpha: web running out of 2
        assert!(text.contains("[0/1]")); // beta: worker stopped
    }

    #[test]
    fn test_projects_panel_running_indicator() {
        let app = sample_app();
        let text = render(40, 10, |f| {
            let area = f.area();
            draw_projects_panel(f, &app, area, false);
        });

        // alpha has a running service (●), beta has none (○).
        assert!(text.contains("●"));
        assert!(text.contains("○"));
    }
}
