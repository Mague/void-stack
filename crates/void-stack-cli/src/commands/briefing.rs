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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil;

    #[test]
    fn test_briefing_schedule_rejects_invalid_times() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();
        assert!(cmd_briefing_schedule(Some("25:00")).is_err());
        assert!(cmd_briefing_schedule(Some("8am")).is_err());
    }

    #[test]
    fn test_briefing_schedule_set_show_and_clear() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();

        cmd_briefing_schedule(Some("08:30")).unwrap();
        let config = load_global_config().unwrap();
        assert_eq!(config.briefing.schedule.as_deref(), Some("08:30"));

        // Show (no argument) must not modify the stored schedule.
        cmd_briefing_schedule(None).unwrap();
        let config = load_global_config().unwrap();
        assert_eq!(config.briefing.schedule.as_deref(), Some("08:30"));

        cmd_briefing_schedule(Some("off")).unwrap();
        let config = load_global_config().unwrap();
        assert_eq!(config.briefing.schedule, None);
    }

    #[test]
    fn test_briefing_active_toggles_membership() {
        let _guard = testutil::config_lock();
        let tmp = tempfile::tempdir().unwrap();
        let name = testutil::unique_name("briefing-active");
        testutil::register_project(&name, tmp.path());

        cmd_briefing_active(&name, "on").unwrap();
        let config = load_global_config().unwrap();
        assert!(config.briefing.active_projects.contains(&name));

        // Turning it on twice must not duplicate the entry.
        cmd_briefing_active(&name, "on").unwrap();
        let config = load_global_config().unwrap();
        assert_eq!(
            config
                .briefing
                .active_projects
                .iter()
                .filter(|n| **n == name)
                .count(),
            1
        );

        cmd_briefing_active(&name, "off").unwrap();
        let config = load_global_config().unwrap();
        assert!(!config.briefing.active_projects.contains(&name));

        assert!(cmd_briefing_active(&name, "banana").is_err());
        assert!(cmd_briefing_active("cli-no-such-project-xyz", "on").is_err());
    }

    #[test]
    fn test_briefing_without_active_projects_errors() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();
        // Force an empty active list so the run is deterministic.
        let mut config = load_global_config().unwrap();
        config.briefing.active_projects.clear();
        save_global_config(&config).unwrap();

        let err = cmd_briefing(false, &[]).unwrap_err();
        assert!(err.to_string().contains("no active projects"), "got: {err}");
    }

    /// An unregistered --project override degrades to a "not found in the
    /// registry" section instead of failing the whole briefing.
    #[test]
    fn test_briefing_with_unknown_project_override_succeeds() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();
        cmd_briefing(false, &["cli-no-such-project-xyz".to_string()]).unwrap();
    }
}
