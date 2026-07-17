use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Wrap};

use void_stack_core::detector::CheckStatus;
use void_stack_core::model::ServiceStatus;

use crate::app::{App, FocusPanel};
use crate::i18n::t;

/// Draw the services tab: projects list + services table + deps + logs.
pub fn draw_services_tab(f: &mut Frame, app: &App, area: Rect) {
    let has_deps = app
        .current_project()
        .map(|p| p.deps_checked)
        .unwrap_or(false);

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

    let top_h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(40)])
        .split(body_v[0]);

    let highlight = app.focus == FocusPanel::Projects;
    super::projects::draw_projects_panel(f, app, top_h[0], highlight);
    draw_services_table(f, app, top_h[1]);

    if has_deps {
        draw_deps_panel(f, app, body_v[1]);
        draw_log_panel(f, app, body_v[2]);
    } else {
        draw_log_panel(f, app, body_v[1]);
    }
}

fn draw_services_table(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let border_color = if app.focus == FocusPanel::Services {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let project_name = app
        .current_project()
        .map(|p| p.name.as_str())
        .unwrap_or("(none)");

    let header_labels = [
        t(l, "th.name"),
        t(l, "th.target"),
        t(l, "th.status"),
        t(l, "th.pid"),
        t(l, "th.uptime"),
        t(l, "th.url"),
    ];
    let header_cells = header_labels.iter().map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    });

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

                let url_str = state.url.as_deref().unwrap_or("-").to_string();

                let row_style = if app.focus == FocusPanel::Services && i == app.selected_service {
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
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
            .title(format!(" {} ({}) ", t(l, "panel.services"), project_name))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );

    f.render_widget(table, area);
}

fn draw_deps_panel(f: &mut Frame, app: &App, area: Rect) {
    let l = app.lang;
    let block = Block::default()
        .title(format!(" {} (d=refresh) ", t(l, "panel.deps")))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let deps = app.current_project().map(|p| &p.deps[..]).unwrap_or(&[]);

    let items: Vec<Span> = deps
        .iter()
        .map(|dep| {
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
        })
        .collect();

    let line = Line::from(items);

    let hint_lines: Vec<Line> = deps
        .iter()
        .filter(|d| !matches!(d.status, CheckStatus::Ok))
        .filter_map(|d| {
            d.fix_hint.as_ref().map(|h| {
                Line::from(vec![
                    Span::styled(
                        format!("  {} ", d.dep_type),
                        Style::default().fg(Color::Yellow),
                    ),
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
    let svc_name = app.selected_service_name().unwrap_or("(none)");

    let border_color = if app.focus == FocusPanel::Logs {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let l = app.lang;
    let title = if app.log_filter_active {
        if let Some(pct) = app.log_filter_savings {
            format!(
                " {}: {} [FILTRADO {:.0}%] ",
                t(l, "panel.logs"),
                svc_name,
                pct
            )
        } else {
            format!(" {}: {} [FILTRADO] ", t(l, "panel.logs"), svc_name)
        }
    } else {
        format!(" {}: {} ", t(l, "panel.logs"), svc_name)
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let raw_logs = app.selected_logs();

    // Apply filter if active
    let filtered;
    let logs: &[String] = if app.log_filter_active {
        filtered = void_stack_core::log_filter::filter_log_lines(raw_logs, true);
        &filtered
    } else {
        raw_logs
    };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::sample_app;
    use crate::i18n::Lang;
    use crate::ui::test_utils::render;
    use void_stack_core::detector::{DependencyStatus, DependencyType};

    #[test]
    fn test_services_tab_shows_projects_table_and_logs() {
        let app = sample_app();
        let text = render(100, 30, |f| {
            let area = f.area();
            draw_services_tab(f, &app, area);
        });

        assert!(text.contains("Proyectos"));
        assert!(text.contains("Servicios (alpha)"));
        // Table headers (Spanish)
        assert!(text.contains("Nombre"));
        assert!(text.contains("Estado"));
        // Fixture services with their statuses
        assert!(text.contains("web"));
        assert!(text.contains("api"));
        assert!(text.contains("RUNNING"));
        assert!(text.contains("STOPPED"));
        // Log panel titled with the selected service
        assert!(text.contains("Logs: web"));
    }

    #[test]
    fn test_services_table_uses_english_labels_when_lang_en() {
        let mut app = sample_app();
        app.lang = Lang::En;
        let text = render(100, 30, |f| {
            let area = f.area();
            draw_services_tab(f, &app, area);
        });

        assert!(text.contains("Services (alpha)"));
        assert!(text.contains("Name"));
        assert!(text.contains("Status"));
    }

    #[test]
    fn test_deps_panel_rendered_after_dependency_check() {
        let mut app = sample_app();
        app.projects[0].deps_checked = true;
        app.projects[0].deps = vec![DependencyStatus::ok(DependencyType::Node)];

        let text = render(100, 30, |f| {
            let area = f.area();
            draw_services_tab(f, &app, area);
        });

        assert!(text.contains("Dependencias"));
        assert!(text.contains("OK"));
    }

    #[test]
    fn test_log_panel_shows_selected_service_log_lines() {
        let mut app = sample_app();
        app.projects[0]
            .logs
            .get_mut("web")
            .unwrap()
            .push("hello from web".to_string());

        let text = render(100, 30, |f| {
            let area = f.area();
            draw_services_tab(f, &app, area);
        });

        assert!(text.contains("hello from web"));
    }

    #[test]
    fn test_log_panel_title_marks_active_filter() {
        let mut app = sample_app();
        app.log_filter_active = true;

        let text = render(100, 30, |f| {
            let area = f.area();
            draw_services_tab(f, &app, area);
        });

        assert!(text.contains("[FILTRADO]"));
    }
}
