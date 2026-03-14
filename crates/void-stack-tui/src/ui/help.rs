use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::App;
use crate::i18n::t;

pub fn draw_help_overlay(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let w = 62.min(area.width);
    let h = 26.min(area.height);
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup_area = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(vec![
            Span::styled(" /", Style::default().fg(Color::Rgb(0, 180, 255))),
            Span::styled("◇", Style::default().fg(Color::Rgb(0, 255, 229))),
            Span::styled("\\", Style::default().fg(Color::Rgb(0, 180, 255))),
            Span::styled(
                " VoidStack TUI",
                Style::default()
                    .fg(Color::Rgb(0, 180, 255))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" - {}", t(l, "help.shortcuts")),
                Style::default().fg(Color::Rgb(0, 180, 255)),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", t(l, "help.navigation")),
            Style::default().fg(Color::Yellow),
        )),
        Line::from(format!("  1-5              {}", t(l, "help.switch_tab"))),
        Line::from(format!("  Tab / Shift+Tab  {}", t(l, "help.switch_panel"))),
        Line::from(format!("  j / Down         {}", t(l, "help.nav_down"))),
        Line::from(format!("  k / Up           {}", t(l, "help.nav_up"))),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", t(l, "help.service_actions")),
            Style::default().fg(Color::Yellow),
        )),
        Line::from(format!(
            "  a / Enter        {}",
            t(l, "help.start_all_svcs")
        )),
        Line::from(format!(
            "  s                {}",
            t(l, "help.start_selected")
        )),
        Line::from(format!("  k                {}", t(l, "help.stop_selected"))),
        Line::from(format!("  K (Shift+k)      {}", t(l, "help.stop_all_svcs"))),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", t(l, "help.analysis_section")),
            Style::default().fg(Color::Yellow),
        )),
        Line::from(format!("  d                {}", t(l, "help.check_deps"))),
        Line::from(format!("  R                {}", t(l, "help.run_action"))),
        Line::from(format!("  L                {}", t(l, "help.toggle_lang"))),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", t(l, "help.other")),
            Style::default().fg(Color::Yellow),
        )),
        Line::from(format!("  l                {}", t(l, "help.go_logs"))),
        Line::from(format!("  Esc              {}", t(l, "help.go_back"))),
        Line::from(format!("  r                {}", t(l, "help.refresh"))),
        Line::from(format!("  q                {}", t(l, "help.quit_hint"))),
        Line::from(format!("  ?                {}", t(l, "help.toggle_help"))),
    ];

    let block = Block::default()
        .title(format!(" {} ", t(l, "help.title")))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(Text::from(help_text))
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, popup_area);
}
