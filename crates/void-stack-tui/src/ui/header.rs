use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

/// Logo: hexagon outline with diamond inside, matching the SVG.
/// Uses Unicode geometric shapes with the brand gradient.
const LOGO_HEX: &str = "\u{2B22}";    // ⬢ filled hexagon (outer shell)
const LOGO_DIAMOND: &str = "\u{25C6}"; // ◆ filled diamond (inner crystal)
const LOGO_CORE: &str = "\u{25CF}";    // ● filled circle (core)

pub fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let session_secs = app.started_time.elapsed().as_secs();
    let session_str = if session_secs < 60 {
        format!("{}s", session_secs)
    } else if session_secs < 3600 {
        format!("{}m", session_secs / 60)
    } else {
        format!("{}h{}m", session_secs / 3600, (session_secs % 3600) / 60)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let status_text = app
        .status_message
        .as_deref()
        .unwrap_or("Ready");

    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(" ", Style::default()),
        // Logo: ⬢◆● — hexagon (cyan) + diamond (turquoise) + core (purple)
        Span::styled(LOGO_HEX, Style::default().fg(Color::Cyan)),
        Span::styled(LOGO_DIAMOND, Style::default().fg(Color::Green)),
        Span::styled(LOGO_CORE, Style::default().fg(Color::Magenta)),
        Span::styled(" VoidStack", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("  [{} projects] [{}/{}] services  session: {}  ",
                app.projects.len(),
                app.total_running(),
                app.total_services(),
                session_str,
            ),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(status_text, Style::default().fg(Color::Yellow)),
    ]))
    .block(block);

    f.render_widget(paragraph, area);
}
