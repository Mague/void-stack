mod analysis;
mod debt;
mod footer;
mod header;
mod help;
pub mod projects;
mod security;
mod services;
mod space;
mod stats;
mod tabs;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use crate::app::{App, AppTab, FocusPanel};

/// Render the entire UI.
pub fn draw(f: &mut Frame, app: &App) {
    let size = f.area();

    // Main vertical layout: header(3) | tab bar(1) | body(fill) | footer(3)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(1), // tab bar
            Constraint::Min(8),    // body
            Constraint::Length(3), // footer
        ])
        .split(size);

    header::draw_header(f, app, outer[0]);
    tabs::draw_tab_bar(f, app, outer[1]);

    match app.active_tab {
        AppTab::Services => services::draw_services_tab(f, app, outer[2]),
        AppTab::Analysis | AppTab::Security | AppTab::Debt | AppTab::Space | AppTab::Stats => {
            draw_with_project_sidebar(f, app, outer[2]);
        }
    }

    footer::draw_footer(f, app, outer[3]);

    if app.show_help {
        help::draw_help_overlay(f, app, size);
    }
}

/// Draw a non-services tab with the project list sidebar on the left.
fn draw_with_project_sidebar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(40)])
        .split(area);

    // Project list is always visible; highlight when Services tab is on Projects focus
    let highlight = app.focus == FocusPanel::Projects && app.active_tab == AppTab::Services;
    projects::draw_projects_panel(f, app, cols[0], highlight);

    // Tab content on the right
    match app.active_tab {
        AppTab::Analysis => analysis::draw_analysis_tab(f, app, cols[1]),
        AppTab::Security => security::draw_security_tab(f, app, cols[1]),
        AppTab::Debt => debt::draw_debt_tab(f, app, cols[1]),
        AppTab::Space => space::draw_space_tab(f, app, cols[1]),
        AppTab::Stats => stats::draw_stats_tab(f, app, cols[1]),
        AppTab::Services => unreachable!(),
    }
}

#[cfg(test)]
pub(crate) mod test_utils {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Render a draw closure into an in-memory terminal and return the buffer
    /// contents as plain text (one line per terminal row).
    pub(crate) fn render(
        width: u16,
        height: u16,
        draw: impl FnOnce(&mut ratatui::Frame),
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
        terminal.draw(draw).expect("draw failed");

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for (i, cell) in buffer.content.iter().enumerate() {
            if i > 0 && i % buffer.area.width as usize == 0 {
                text.push('\n');
            }
            text.push_str(cell.symbol());
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::test_utils::render;
    use super::*;
    use crate::app::test_support::sample_app;

    #[test]
    fn test_draw_renders_every_tab_without_panic() {
        for tab in [
            AppTab::Services,
            AppTab::Analysis,
            AppTab::Security,
            AppTab::Debt,
            AppTab::Space,
            AppTab::Stats,
        ] {
            let mut app = sample_app();
            app.active_tab = tab;
            let text = render(100, 30, |f| draw(f, &app));
            assert!(text.contains("VoidStack"), "header missing for {tab:?}");
        }
    }

    #[test]
    fn test_draw_non_services_tab_keeps_project_sidebar() {
        let mut app = sample_app();
        app.active_tab = AppTab::Security;
        let text = render(100, 30, |f| draw(f, &app));
        assert!(text.contains("Proyectos"));
        assert!(text.contains("alpha"));
        // Security run hint is shown when no audit has been run yet.
        assert!(text.contains("auditoria de seguridad"));
    }

    #[test]
    fn test_draw_shows_help_overlay_when_enabled() {
        let mut app = sample_app();
        app.show_help = true;
        let text = render(100, 30, |f| draw(f, &app));
        assert!(text.contains("Atajos de Teclado"));
    }
}
