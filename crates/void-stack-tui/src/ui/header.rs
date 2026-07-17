use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::i18n::t;

// Brand colors matching the SVG gradient: cyan → turquoise → purple
const CYAN: Color = Color::Rgb(0, 180, 255);
const TURQUOISE: Color = Color::Rgb(0, 255, 229);
const PURPLE: Color = Color::Rgb(168, 85, 247);

/// Draw the header with a 3-line ASCII logo matching the SVG hexagon+diamond+core.
pub fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let session_secs = app.started_time.elapsed().as_secs();
    let session_str = if session_secs < 60 {
        format!("{}s", session_secs)
    } else if session_secs < 3600 {
        format!("{}m", session_secs / 60)
    } else {
        format!("{}h{}m", session_secs / 3600, (session_secs % 3600) / 60)
    };

    let l = app.lang;
    let status_text = app.status_message.as_deref().unwrap_or(t(l, "ready"));

    // Split header: logo area (left, fixed) | info area (right, fill)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(12), Constraint::Min(40)])
        .split(area);

    // ── Logo (3 lines, matching SVG hexagon + diamond + core) ──
    //   ╱◇╲      line 0: hex top + diamond top
    //  ⬡ ● ⬡     line 1: hex sides + core
    //   ╲◇╱      line 2: hex bottom + diamond bottom
    let logo_lines = vec![
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("/", Style::default().fg(CYAN)),
            Span::styled("◇", Style::default().fg(TURQUOISE)),
            Span::styled("\\", Style::default().fg(CYAN)),
        ]),
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled("⬡", Style::default().fg(CYAN)),
            Span::styled("●", Style::default().fg(PURPLE)),
            Span::styled("⬡", Style::default().fg(CYAN)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("\\", Style::default().fg(CYAN)),
            Span::styled("◇", Style::default().fg(TURQUOISE)),
            Span::styled("/", Style::default().fg(CYAN)),
        ]),
    ];

    let logo = Paragraph::new(Text::from(logo_lines));
    f.render_widget(logo, cols[0]);

    // ── Info panel (right side) ──
    let info_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let info = Paragraph::new(Line::from(vec![
        Span::styled(
            "VoidStack",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  [{} {}] [{}/{}] {}  {}: {}  ",
                app.projects.len(),
                t(l, "projects"),
                app.total_running(),
                app.total_services(),
                t(l, "services"),
                t(l, "session"),
                session_str,
            ),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(status_text, Style::default().fg(Color::Yellow)),
    ]))
    .block(info_block);

    f.render_widget(info, cols[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::sample_app;
    use crate::ui::test_utils::render;

    #[test]
    fn test_header_shows_brand_counts_and_ready_status() {
        let app = sample_app();
        let text = render(100, 3, |f| {
            let area = f.area();
            draw_header(f, &app, area);
        });

        assert!(text.contains("VoidStack"));
        assert!(text.contains("[2 proyectos]"));
        assert!(text.contains("[1/3] servicios"));
        // No status message set: shows the localized "ready" text.
        assert!(text.contains("Listo"));
    }

    #[test]
    fn test_header_shows_status_message_when_set() {
        let mut app = sample_app();
        app.status_message = Some("Working on it".to_string());
        let text = render(100, 3, |f| {
            let area = f.area();
            draw_header(f, &app, area);
        });

        assert!(text.contains("Working on it"));
        assert!(!text.contains("Listo"));
    }
}
