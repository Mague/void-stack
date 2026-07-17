#[tauri::command]
pub fn get_token_stats_cmd(project: Option<String>, days: Option<u32>) -> Result<String, String> {
    let report = void_stack_core::stats::get_stats(project.as_deref(), days.unwrap_or(30))
        .map_err(|e| format!("Error loading stats: {}", e))?;

    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_token_stats_returns_valid_json() {
        // Isolate the data dir so the stats DB is created inside a tempdir.
        let _g = crate::commands::test_support::config_guard();
        let json = get_token_stats_cmd(None, Some(7)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_object());
    }
}
