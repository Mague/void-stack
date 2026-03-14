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
