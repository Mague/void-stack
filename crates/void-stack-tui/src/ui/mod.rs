mod analysis;
mod debt;
mod footer;
mod header;
mod help;
pub mod projects;
mod security;
mod services;
mod space;
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
        AppTab::Analysis | AppTab::Security | AppTab::Debt | AppTab::Space => {
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
        AppTab::Services => unreachable!(),
    }
}
