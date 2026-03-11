use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, AppTab, FocusPanel};

pub fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let keys = match app.active_tab {
        AppTab::Services => match app.focus {
            FocusPanel::Projects => {
                " 1-5: Tabs | Tab: Panel | j/k: Select | a: Start All | K: Stop All | d: Deps | q: Quit | ?: Help "
            }
            FocusPanel::Services => {
                " 1-5: Tabs | Tab: Panel | s: Start | k: Stop | a: All | K: Stop All | d: Deps | l: Logs | ?: Help "
            }
            FocusPanel::Logs => {
                " 1-5: Tabs | Tab: Panel | Esc: Services | Up/Down: Scroll | q: Quit | ?: Help "
            }
        },
        AppTab::Analysis => {
            " 1-5: Tabs | R: Run Analysis | j/k: Scroll | q: Quit | ?: Help "
        }
        AppTab::Security => {
            " 1-5: Tabs | R: Run Audit | j/k: Scroll | q: Quit | ?: Help "
        }
        AppTab::Debt => {
            " 1-5: Tabs | R: Scan Debt | j/k: Scroll | q: Quit | ?: Help "
        }
        AppTab::Space => {
            " 1-5: Tabs | R: Scan Space | j/k: Scroll | q: Quit | ?: Help "
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
