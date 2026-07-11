//! `void briefing` — consolidated daily report for the active projects.

use anyhow::Result;
use void_stack_core::briefing;
use void_stack_core::global_config::{find_project, load_global_config, save_global_config};

pub fn cmd_briefing(save: bool, projects: &[String]) -> Result<()> {
    let config = load_global_config()?;
    let only = if projects.is_empty() {
        None
    } else {
        Some(projects)
    };
    let md = briefing::generate_briefing(&config, only.map(|p| p as &[String]))
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("{}", md);

    if save || config.briefing.save {
        let path = briefing::save_briefing(&md, chrono::Local::now().date_naive())
            .map_err(|e| anyhow::anyhow!(e))?;
        eprintln!("✓ saved to {}", path.display());
    }
    Ok(())
}

/// Toggle a project in the briefing's active list.
pub fn cmd_briefing_active(project_name: &str, state: &str) -> Result<()> {
    let mut config = load_global_config()?;
    let canonical = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .name
        .clone();

    let on = match state.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" => true,
        "off" | "false" | "no" => false,
        other => anyhow::bail!("expected on|off, got '{}'", other),
    };

    let list = &mut config.briefing.active_projects;
    let present = list.iter().any(|n| n.eq_ignore_ascii_case(&canonical));
    match (on, present) {
        (true, false) => list.push(canonical.clone()),
        (false, true) => list.retain(|n| !n.eq_ignore_ascii_case(&canonical)),
        _ => {}
    }
    save_global_config(&config)?;
    println!(
        "✓ '{}' is {} the daily briefing ({} active)",
        canonical,
        if on { "in" } else { "out of" },
        config.briefing.active_projects.len()
    );
    Ok(())
}

/// Set, show or clear the daemon schedule ("HH:MM" or "off").
pub fn cmd_briefing_schedule(time: Option<&str>) -> Result<()> {
    let mut config = load_global_config()?;
    match time {
        None => match &config.briefing.schedule {
            Some(at) => println!("briefing scheduled daily at {}", at),
            None => println!("no briefing schedule set"),
        },
        Some("off") => {
            config.briefing.schedule = None;
            save_global_config(&config)?;
            println!("✓ briefing schedule cleared");
        }
        Some(at) => {
            if chrono::NaiveTime::parse_from_str(at, "%H:%M").is_err() {
                anyhow::bail!("expected HH:MM (24h) or 'off', got '{}'", at);
            }
            config.briefing.schedule = Some(at.to_string());
            save_global_config(&config)?;
            println!(
                "✓ briefing scheduled daily at {} (runs inside `void daemon`)",
                at
            );
        }
    }
    Ok(())
}
