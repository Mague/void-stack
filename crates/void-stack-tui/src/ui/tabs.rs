use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, AppTab};
use crate::i18n::t;

/// Render the tab bar at the top of the body area.
pub fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let lang = app.lang;
    let tabs = [
        (AppTab::Services, format!("1:{}", t(lang, "tab.services"))),
        (AppTab::Analysis, format!("2:{}", t(lang, "tab.analysis"))),
        (AppTab::Security, format!("3:{}", t(lang, "tab.security"))),
        (AppTab::Debt, format!("4:{}", t(lang, "tab.debt"))),
        (AppTab::Space, format!("5:{}", t(lang, "tab.space"))),
        (AppTab::Stats, format!("6:{}", t(lang, "tab.stats"))),
    ];

    let mut spans = vec![Span::styled(" ", Style::default())];

    for (i, (tab, label)) in tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        }

        let style = if app.active_tab == *tab {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Gray)
        };

        spans.push(Span::styled(label.clone(), style));
    }

    // Language indicator
    spans.push(Span::styled("  ", Style::default()));
    spans.push(Span::styled(
        format!("[{}]", lang.code()),
        Style::default().fg(Color::DarkGray),
    ));

    let paragraph = Paragraph::new(Line::from(spans));
    f.render_widget(paragraph, area);
}
