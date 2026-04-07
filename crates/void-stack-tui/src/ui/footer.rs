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
