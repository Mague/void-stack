use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap,
};

use devlaunch_core::model::ServiceStatus;

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
    let running = app.running_count();
    let total = app.total_count();

    let session_secs = app.started_time.elapsed().as_secs();
    let session_str = if session_secs < 60 {
        format!("{}s", session_secs)
    } else if session_secs < 3600 {
        format!("{}m", session_secs / 60)
    } else {
        format!("{}h{}m", session_secs / 3600, (session_secs % 3600) / 60)
    };

    let title = format!(
        " DevLaunch  {}  [{}/{}] running  session: {} ",
        app.project_name, running, total, session_str,
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
    // Split body into services table (top) and log panel (bottom)
    let body_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    draw_services_table(f, app, body_layout[0]);
    draw_log_panel(f, app, body_layout[1]);
}

fn draw_services_table(f: &mut Frame, app: &App, area: Rect) {
    let highlight_style = if app.focus == FocusPanel::Services {
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(Color::DarkGray)
    };

    let header_cells = ["Name", "Target", "Status", "PID", "Uptime"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).bold()));

    let header = Row::new(header_cells).height(1);

    let now = Utc::now();

    let rows: Vec<Row> = app
        .states
        .iter()
        .enumerate()
        .map(|(i, state)| {
            let target_str = app
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

            let row_style = if i == app.selected {
                highlight_style
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(state.service_name.clone()),
                Cell::from(target_str),
                Cell::from(state.status.to_string()).style(status_style),
                Cell::from(pid_str),
                Cell::from(uptime_str),
            ])
            .style(row_style)
        })
        .collect();

    let border_color = if app.focus == FocusPanel::Services {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(25),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(" Services ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );

    f.render_widget(table, area);
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

    // Visible height inside the block border
    let inner_height = area.height.saturating_sub(2) as usize;

    // Auto-scroll to bottom unless user has scrolled up
    let total = logs.len();
    let effective_scroll = if app.log_scroll == 0 {
        // Auto-scroll: show the last `inner_height` lines
        total.saturating_sub(inner_height)
    } else {
        app.log_scroll.min(total.saturating_sub(inner_height))
    };

    let visible: Vec<ListItem> = logs
        .iter()
        .skip(effective_scroll)
        .take(inner_height)
        .map(|line| {
            let style = if line.starts_with("[devlaunch]") {
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
    let keys = if app.focus == FocusPanel::Logs {
        " ESC: Services | Up/Down: Scroll | q: Quit | ?: Help "
    } else {
        " a/Enter: Start All | s: Start | k: Stop | K: Stop All | j/Down: Down | k/Up: Up | l: Logs | r: Refresh | q: Quit | ?: Help "
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
    // Centered overlay
    let w = 52.min(area.width);
    let h = 18.min(area.height);
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup_area = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(Span::styled(
            "  DevLaunch TUI - Keyboard Shortcuts",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(""),
        Line::from("  a / Enter   Start all services"),
        Line::from("  s           Start selected service"),
        Line::from("  k           Stop selected service"),
        Line::from("  K (Shift)   Stop all services"),
        Line::from("  j / Down    Move selection down"),
        Line::from("  Up          Move selection up"),
        Line::from("  l           View logs (switch panel)"),
        Line::from("  r           Refresh process status"),
        Line::from("  q           Quit (stops all services)"),
        Line::from("  ?           Toggle this help"),
        Line::from("  Esc         Close help / back to services"),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
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
