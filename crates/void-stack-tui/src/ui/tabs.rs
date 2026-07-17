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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::sample_app;
    use crate::i18n::Lang;
    use crate::ui::test_utils::render;

    #[test]
    fn test_tab_bar_shows_all_tabs_and_lang_indicator_spanish() {
        let app = sample_app();
        let text = render(100, 1, |f| {
            let area = f.area();
            draw_tab_bar(f, &app, area);
        });

        assert!(text.contains("1:Servicios"));
        assert!(text.contains("2:Analisis"));
        assert!(text.contains("3:Seguridad"));
        assert!(text.contains("4:Deuda"));
        assert!(text.contains("5:Espacio"));
        assert!(text.contains("6:Stats"));
        assert!(text.contains("[ES]"));
    }

    #[test]
    fn test_tab_bar_switches_labels_in_english() {
        let mut app = sample_app();
        app.lang = Lang::En;
        app.active_tab = AppTab::Debt;
        let text = render(100, 1, |f| {
            let area = f.area();
            draw_tab_bar(f, &app, area);
        });

        assert!(text.contains("1:Services"));
        assert!(text.contains("4:Debt"));
        assert!(text.contains("[EN]"));
    }
}
