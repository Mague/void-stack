use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub fn draw_help_overlay(f: &mut Frame, area: Rect) {
    let w = 60.min(area.width);
    let h = 24.min(area.height);
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup_area = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("\u{2B22}", Style::default().fg(Color::Cyan)),
            Span::styled("\u{25C6}", Style::default().fg(Color::Green)),
            Span::styled("\u{25CF}", Style::default().fg(Color::Magenta)),
            Span::styled(" VoidStack TUI", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled(" - Keyboard Shortcuts", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Navigation:", Style::default().fg(Color::Yellow))),
        Line::from("  1-5              Switch tab (Services/Analysis/Security/Debt/Space)"),
        Line::from("  Tab / Shift+Tab  Switch panel (Projects/Services/Logs)"),
        Line::from("  j / Down         Move selection down"),
        Line::from("  k / Up           Move selection up"),
        Line::from(""),
        Line::from(Span::styled("  Service Actions:", Style::default().fg(Color::Yellow))),
        Line::from("  a / Enter        Start all services (current project)"),
        Line::from("  s                Start selected service"),
        Line::from("  k                Stop selected service (Services panel)"),
        Line::from("  K (Shift+k)      Stop all services (current project)"),
        Line::from(""),
        Line::from(Span::styled("  Analysis:", Style::default().fg(Color::Yellow))),
        Line::from("  d                Check dependencies"),
        Line::from("  R                Run analysis / audit / scan (on current tab)"),
        Line::from(""),
        Line::from(Span::styled("  Other:", Style::default().fg(Color::Yellow))),
        Line::from("  l                Switch to Logs panel"),
        Line::from("  Esc              Back to Services panel"),
        Line::from("  r                Refresh status"),
        Line::from("  q                Quit (stops all running services)"),
        Line::from("  ?                Toggle this help"),
    ];

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(Text::from(help_text))
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, popup_area);
}
