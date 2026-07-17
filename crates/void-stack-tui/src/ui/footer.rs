use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, AppTab, FocusPanel};
use crate::i18n::t;

pub fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let keys = match app.active_tab {
        AppTab::Services => match app.focus {
            FocusPanel::Projects => {
                format!(
                    " 1-6: {} | Tab: {} | j/k: {} | a: {} | K: {} | d: {} | G: .claudeignore | L: {} | q: {} | ?: {} ",
                    t(l, "footer.tabs"),
                    t(l, "footer.panel"),
                    t(l, "footer.select"),
                    t(l, "footer.start_all"),
                    t(l, "footer.stop_all"),
                    t(l, "footer.deps"),
                    t(l, "footer.lang"),
                    t(l, "footer.quit"),
                    t(l, "footer.help")
                )
            }
            FocusPanel::Services => {
                format!(
                    " 1-6: {} | Tab: {} | s: {} | k: {} | a: {} | K: {} | d: {} | l: {} | ?: {} ",
                    t(l, "footer.tabs"),
                    t(l, "footer.panel"),
                    t(l, "footer.start"),
                    t(l, "footer.stop"),
                    t(l, "footer.start_all"),
                    t(l, "footer.stop_all"),
                    t(l, "footer.deps"),
                    t(l, "footer.logs"),
                    t(l, "footer.help")
                )
            }
            FocusPanel::Logs => {
                format!(
                    " 1-6: {} | Tab: {} | f: Filter | Esc: {} | Up/Down: {} | q: {} | ?: {} ",
                    t(l, "footer.tabs"),
                    t(l, "footer.panel"),
                    t(l, "panel.services"),
                    t(l, "footer.scroll"),
                    t(l, "footer.quit"),
                    t(l, "footer.help")
                )
            }
        },
        _ => {
            format!(
                " 1-6: {} | R: {} | j/k: {} | L: {} | q: {} | ?: {} ",
                t(l, "footer.tabs"),
                t(l, "footer.run"),
                t(l, "footer.select"),
                t(l, "footer.lang"),
                t(l, "footer.quit"),
                t(l, "footer.help")
            )
        }
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(Line::from(Span::styled(
        keys,
        Style::default().fg(Color::Gray),
    )))
    .block(block);

    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::sample_app;
    use crate::i18n::Lang;
    use crate::ui::test_utils::render;

    fn footer_text(app: &App) -> String {
        // Wide enough that no shortcut list gets truncated.
        render(170, 3, |f| {
            let area = f.area();
            draw_footer(f, app, area);
        })
    }

    #[test]
    fn test_footer_projects_focus_shows_project_shortcuts() {
        let app = sample_app(); // focus starts on Projects
        let text = footer_text(&app);
        assert!(text.contains("a: Iniciar Todo"));
        assert!(text.contains("K: Detener Todo"));
        assert!(text.contains("G: .claudeignore"));
        assert!(text.contains("q: Salir"));
    }

    #[test]
    fn test_footer_services_focus_shows_service_shortcuts() {
        let mut app = sample_app();
        app.focus = FocusPanel::Services;
        let text = footer_text(&app);
        assert!(text.contains("s: Iniciar"));
        assert!(text.contains("k: Detener"));
        assert!(text.contains("l: Logs"));
    }

    #[test]
    fn test_footer_logs_focus_shows_filter_and_scroll() {
        let mut app = sample_app();
        app.focus = FocusPanel::Logs;
        let text = footer_text(&app);
        assert!(text.contains("f: Filter"));
        assert!(text.contains("Up/Down: Scroll"));
    }

    #[test]
    fn test_footer_non_services_tab_shows_run_shortcut() {
        let mut app = sample_app();
        app.active_tab = AppTab::Space;
        let text = footer_text(&app);
        assert!(text.contains("R: Ejecutar"));

        app.lang = Lang::En;
        let text = footer_text(&app);
        assert!(text.contains("R: Run"));
        assert!(text.contains("q: Quit"));
    }
}
