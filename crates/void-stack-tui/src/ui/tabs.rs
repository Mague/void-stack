use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, AppTab};

/// Render the tab bar at the top of the body area.
pub fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let tabs = [
        (AppTab::Services, "1:Services"),
        (AppTab::Analysis, "2:Analysis"),
        (AppTab::Security, "3:Security"),
        (AppTab::Debt, "4:Debt"),
        (AppTab::Space, "5:Space"),
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

        spans.push(Span::styled(*label, style));
    }

    let paragraph = Paragraph::new(Line::from(spans));
    f.render_widget(paragraph, area);
}
