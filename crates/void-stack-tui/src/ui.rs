use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap,
};

use void_stack_core::detector::CheckStatus;
use void_stack_core::model::ServiceStatus;

use crate::app::{App, FocusPanel};

/// Render the entire UI.
pub fn draw(f: &mut Frame, app: &App) {
    let size = f.area();

    // Main vertical layout: header(3) | body(fill) | footer(3)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(8),    // body
            Constraint::Length(3), // footer
        ])
        .split(size);

    draw_header(f, app, outer[0]);
    draw_body(f, app, outer[1]);
    draw_footer(f, app, outer[2]);

    if app.show_help {
        draw_help_overlay(f, size);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let session_secs = app.started_time.elapsed().as_secs();
    let session_str = if session_secs < 60 {
        format!("{}s", session_secs)
    } else if session_secs < 3600 {
        format!("{}m", session_secs / 60)
    } else {
        format!("{}h{}m", session_secs / 3600, (session_secs % 3600) / 60)
    };

    let title = format!(
        " VoidStack  [{} projects] [{}/{}] services  session: {} ",
        app.projects.len(),
        app.total_running(),
        app.total_services(),
        session_str,
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let status_text = app
        .status_message
        .as_deref()
        .unwrap_or("Ready");

    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(status_text, Style::default().fg(Color::Yellow)),
    ]))
    .block(block);

    f.render_widget(paragraph, area);
}

fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let has_deps = app.current_project().map(|p| p.deps_checked).unwrap_or(false);

    // Body: top (projects + services) | deps (if checked) | bottom (logs)
    let body_v = if has_deps {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Length(6),
                Constraint::Min(6),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area)
    };

    // Top: projects list (left) | services table (right)
    let top_h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(40)])
        .split(body_v[0]);

    draw_projects_panel(f, app, top_h[0]);
    draw_services_table(f, app, top_h[1]);

    if has_deps {
        draw_deps_panel(f, app, body_v[1]);
        draw_log_panel(f, app, body_v[2]);
    } else {
        draw_log_panel(f, app, body_v[1]);
    }
}

fn draw_projects_panel(f: &mut Frame, app: &App, area: Rect) {
    let border_color = if app.focus == FocusPanel::Projects {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(" Projects ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let items: Vec<ListItem> = app
        .projects
        .iter()
        .enumerate()
        .map(|(i, project)| {
            let running = project
                .states
                .iter()
                .filter(|s| s.status == ServiceStatus::Running)
                .count();
            let total = project.service_names.len();

            let indicator = if running > 0 { "●" } else { "○" };
            let indicator_color = if running > 0 { Color::Green } else { Color::DarkGray };

            let style = if i == app.selected_project {
                Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", indicator), Style::default().fg(indicator_color)),
                Span::styled(
                    format!("{} ", project.name),
                    style,
                ),
                Span::styled(
                    format!("[{}/{}]", running, total),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
            .style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_services_table(f: &mut Frame, app: &App, area: Rect) {
    let border_color = if app.focus == FocusPanel::Services {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let project_name = app
        .current_project()
        .map(|p| p.name.as_str())
        .unwrap_or("(none)");

    let header_cells = ["Name", "Target", "Status", "PID", "Uptime", "URL"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).bold()));

    let header = Row::new(header_cells).height(1);

    let now = Utc::now();

    let rows: Vec<Row> = if let Some(project) = app.current_project() {
        project
            .states
            .iter()
            .enumerate()
            .map(|(i, state)| {
                let target_str = project
                    .service_targets
                    .get(&state.service_name)
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "?".to_string());

                let status_style = match state.status {
                    ServiceStatus::Running => Style::default().fg(Color::Green),
                    ServiceStatus::Starting => Style::default().fg(Color::Yellow),
                    ServiceStatus::Stopping => Style::default().fg(Color::Yellow),
                    ServiceStatus::Failed => Style::default().fg(Color::Red),
                    ServiceStatus::Stopped => Style::default().fg(Color::Gray),
                };

                let pid_str = state
                    .pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "-".to_string());

                let uptime_str = match (state.status, state.started_at) {
                    (ServiceStatus::Running, Some(started)) => {
                        let dur = now.signed_duration_since(started);
                        let secs = dur.num_seconds().max(0);
                        if secs < 60 {
                            format!("{}s", secs)
                        } else if secs < 3600 {
                            format!("{}m {}s", secs / 60, secs % 60)
                        } else {
                            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
                        }
                    }
                    _ => "-".to_string(),
                };

                let url_str = state
                    .url
                    .as_deref()
                    .unwrap_or("-")
                    .to_string();

                let row_style = if app.focus == FocusPanel::Services && i == app.selected_service {
                    Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                Row::new(vec![
                    Cell::from(state.service_name.clone()),
                    Cell::from(target_str),
                    Cell::from(state.status.to_string()).style(status_style),
                    Cell::from(pid_str),
                    Cell::from(uptime_str),
                    Cell::from(url_str).style(Style::default().fg(Color::Blue)),
                ])
                .style(row_style)
            })
            .collect()
    } else {
        vec![]
    };

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(18),
            Constraint::Percentage(10),
            Constraint::Percentage(12),
            Constraint::Percentage(10),
            Constraint::Percentage(12),
            Constraint::Percentage(38),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(format!(" Services ({}) ", project_name))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );

    f.render_widget(table, area);
}

fn draw_deps_panel(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Dependencies (d=refresh) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let deps = app.current_project()
        .map(|p| &p.deps[..])
        .unwrap_or(&[]);

    let items: Vec<Span> = deps.iter().map(|dep| {
        let (icon, color) = match dep.status {
            CheckStatus::Ok => ("OK", Color::Green),
            CheckStatus::Missing => ("MISS", Color::Red),
            CheckStatus::NotRunning => ("DOWN", Color::Yellow),
            CheckStatus::NeedsSetup => ("SETUP", Color::Yellow),
            CheckStatus::Unknown => ("?", Color::DarkGray),
        };

        let ver = dep.version.as_deref().unwrap_or("");
        let text = format!(" {} {} {} ", icon, dep.dep_type, ver);
        Span::styled(text, Style::default().fg(color))
    }).collect();

    // Show deps inline as a horizontal list
    let line = Line::from(items);

    let hint_lines: Vec<Line> = deps.iter()
        .filter(|d| !matches!(d.status, CheckStatus::Ok))
        .filter_map(|d| {
            d.fix_hint.as_ref().map(|h| {
                Line::from(vec![
                    Span::styled(format!("  {} ", d.dep_type), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("fix: {}", h), Style::default().fg(Color::DarkGray)),
                ])
            })
        })
        .collect();

    let mut text = vec![line];
    if !hint_lines.is_empty() {
        text.push(Line::from(""));
        text.extend(hint_lines);
    }

    let paragraph = Paragraph::new(Text::from(text))
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_log_panel(f: &mut Frame, app: &App, area: Rect) {
    let svc_name = app
        .selected_service_name()
        .unwrap_or("(none)");

    let border_color = if app.focus == FocusPanel::Logs {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(format!(" Logs: {} ", svc_name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let logs = app.selected_logs();

    let inner_height = area.height.saturating_sub(2) as usize;

    let total = logs.len();
    let effective_scroll = if app.log_scroll == 0 {
        total.saturating_sub(inner_height)
    } else {
        app.log_scroll.min(total.saturating_sub(inner_height))
    };

    let visible: Vec<ListItem> = logs
        .iter()
        .skip(effective_scroll)
        .take(inner_height)
        .map(|line| {
            let style = if line.starts_with("[void-stack]") {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(line.as_str(), style)))
        })
        .collect();

    let list = List::new(visible).block(block);
    f.render_widget(list, area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let keys = match app.focus {
        FocusPanel::Projects => {
            " Tab: Next | j/k: Select | a: Start All | K: Stop All | d: Check Deps | q: Quit | ?: Help "
        }
        FocusPanel::Services => {
            " Tab: Next | s: Start | k: Stop | a: Start All | K: Stop All | d: Check Deps | l: Logs | ?: Help "
        }
        FocusPanel::Logs => {
            " Tab: Next Panel | Esc: Services | Up/Down: Scroll | q: Quit | ?: Help "
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

fn draw_help_overlay(f: &mut Frame, area: Rect) {
    let w = 56.min(area.width);
    let h = 20.min(area.height);
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup_area = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(Span::styled(
            "  VoidStack TUI - Keyboard Shortcuts",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(""),
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
        Line::from(Span::styled("  Other:", Style::default().fg(Color::Yellow))),
        Line::from("  d                Check dependencies"),
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
