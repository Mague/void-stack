//! Intelligent log output filtering — reduces noise from service logs.
//!
//! Strategies applied in order:
//! 1. Strip ANSI escape codes
//! 2. Deduplicate consecutive repeated lines → "message (×N)"
//! 3. Remove progress bars and download indicators
//! 4. If compact=true: filter by log level (keep only WARN/ERROR)
//! 5. Truncate long output (first 20 + last 30, middle omitted)

mod dedup;
mod rules;
mod truncate;

pub use rules::strip_ansi;

use dedup::deduplicate;
use rules::{has_log_levels, is_low_priority_level, is_progress_line};
use truncate::truncate_lines;

/// Result of filtering log output.
#[derive(Debug, Clone)]
pub struct FilterResult {
    /// The filtered content.
    pub content: String,
    /// Number of lines in the original input.
    pub lines_original: usize,
    /// Number of lines in the filtered output.
    pub lines_filtered: usize,
    /// Token savings percentage (0.0 - 100.0).
    pub savings_pct: f32,
}

/// Filter log output with intelligent noise reduction.
///
/// - `compact`: if true, filter out INFO/DEBUG lines when log levels are detected.
pub fn filter_log_output(raw: &str, compact: bool) -> FilterResult {
    // Fallback: if input is empty, return as-is
    if raw.is_empty() {
        return FilterResult {
            content: String::new(),
            lines_original: 0,
            lines_filtered: 0,
            savings_pct: 0.0,
        };
    }

    let original_lines: Vec<&str> = raw.lines().collect();
    let lines_original = original_lines.len();

    // Catch panics — on any error, return raw
    let filtered = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        apply_filters(&original_lines, compact)
    })) {
        Ok(result) => result,
        Err(_) => original_lines.iter().map(|s| s.to_string()).collect(),
    };

    let lines_filtered = filtered.len();
    let savings_pct = if lines_original > 0 {
        (1.0 - (lines_filtered as f32 / lines_original as f32)) * 100.0
    } else {
        0.0
    };

    FilterResult {
        content: filtered.join("\n"),
        lines_original,
        lines_filtered,
        savings_pct: savings_pct.max(0.0),
    }
}

/// Filter log output and record savings to stats DB.
pub fn filter_log_output_tracked(raw: &str, compact: bool, project: &str) -> FilterResult {
    let result = filter_log_output(raw, compact);
    if result.savings_pct > 10.0 {
        crate::stats::record_saving(crate::stats::TokenSavingsRecord {
            timestamp: chrono::Utc::now(),
            project: project.to_string(),
            operation: "log_filter".to_string(),
            lines_original: result.lines_original,
            lines_filtered: result.lines_filtered,
            savings_pct: result.savings_pct,
        });
    }
    result
}

/// Filter a Vec of log lines (already split). Returns filtered lines.
pub fn filter_log_lines(lines: &[String], compact: bool) -> Vec<String> {
    if lines.is_empty() {
        return vec![];
    }
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    apply_filters(&refs, compact)
}

fn apply_filters(lines: &[&str], compact: bool) -> Vec<String> {
    // 1. Strip ANSI
    let stripped: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();

    // 2. Remove progress bars
    let no_progress: Vec<String> = stripped
        .into_iter()
        .filter(|l| !is_progress_line(l))
        .collect();

    // 3. Deduplicate consecutive lines
    let deduped = deduplicate(&no_progress);

    // 4. Compact mode: filter by level
    let leveled = if compact && has_log_levels(&deduped) {
        deduped
            .into_iter()
            .filter(|l| !is_low_priority_level(l))
            .collect()
    } else {
        deduped
    };

    // 5. Truncate if too long
    truncate_lines(leveled)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Level filtering (compact pipeline) ──────────────────

    #[test]
    fn test_compact_keeps_warn_error() {
        let lines = vec![
            "[INFO] Starting server",
            "[DEBUG] Loading config",
            "[WARN] Deprecated API used",
            "[ERROR] Connection refused",
            "[INFO] Request received",
        ];
        let result = apply_filters(&lines, true);
        assert_eq!(result.len(), 2);
        assert!(result[0].contains("WARN"));
        assert!(result[1].contains("ERROR"));
    }

    #[test]
    fn test_non_compact_keeps_all_levels() {
        let lines = vec![
            "[INFO] Starting server",
            "[DEBUG] Loading config",
            "[WARN] Deprecated API used",
        ];
        let result = apply_filters(&lines, false);
        assert_eq!(result.len(), 3);
    }

    // ── Full filter pipeline ────────────────────────────────

    #[test]
    fn test_filter_empty() {
        let result = filter_log_output("", false);
        assert_eq!(result.lines_original, 0);
        assert_eq!(result.lines_filtered, 0);
        assert_eq!(result.savings_pct, 0.0);
    }

    #[test]
    fn test_filter_basic() {
        let input = "line 1\nline 2\nline 3";
        let result = filter_log_output(input, false);
        assert_eq!(result.lines_original, 3);
        assert_eq!(result.lines_filtered, 3);
    }

    #[test]
    fn test_filter_strips_ansi() {
        let input = "\x1b[32mhello\x1b[0m\n\x1b[31mworld\x1b[0m";
        let result = filter_log_output(input, false);
        assert_eq!(result.content, "hello\nworld");
    }

    #[test]
    fn test_filter_removes_progress() {
        let input = "Starting server...\n████████░░░░ 65%\nServer ready on :3000";
        let result = filter_log_output(input, false);
        assert_eq!(result.lines_filtered, 2);
        assert!(!result.content.contains("████"));
    }

    #[test]
    fn test_filter_deduplicates() {
        let input = "heartbeat ok\nheartbeat ok\nheartbeat ok\nheartbeat ok\ndone";
        let result = filter_log_output(input, false);
        assert!(result.content.contains("(×4)"));
        assert_eq!(result.lines_filtered, 2);
    }

    #[test]
    fn test_filter_savings() {
        let input = "a\na\na\na\na\na\na\na\na\na";
        let result = filter_log_output(input, false);
        assert!(result.savings_pct > 80.0);
    }

    // ── Framework-specific fixtures ─────────────────────────

    #[test]
    fn test_fixture_vite() {
        let input = "\x1b[32m  VITE v5.0.0\x1b[0m  ready in 250 ms\n\n\
                     \x1b[32m  ➜\x1b[0m  \x1b[1mLocal:\x1b[0m   http://localhost:5173/\n\
                     \x1b[32m  ➜\x1b[0m  \x1b[1mNetwork:\x1b[0m use --host to expose\n\
                     \x1b[36m12:00:01\x1b[0m [vite] page reload src/App.tsx\n\
                     \x1b[36m12:00:01\x1b[0m [vite] page reload src/App.tsx\n\
                     \x1b[36m12:00:01\x1b[0m [vite] page reload src/App.tsx";
        let result = filter_log_output(input, false);
        assert!(result.content.contains("VITE v5.0.0"));
        assert!(result.content.contains("localhost:5173"));
        // Repeated reload lines should be deduped
        assert!(result.content.contains("(×3)"));
    }

    #[test]
    fn test_fixture_cargo_run() {
        let input = "   Compiling myapp v0.1.0\n\
                     Downloading 5 crates...\n\
                     ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
                     \x1b[32m    Finished\x1b[0m dev [unoptimized + debuginfo]\n\
                     \x1b[32m     Running\x1b[0m `target/debug/myapp`\n\
                     Listening on http://0.0.0.0:8080";
        let result = filter_log_output(input, false);
        assert!(!result.content.contains("━━━━"));
        assert!(!result.content.contains("Downloading 5 crates"));
        assert!(result.content.contains("Compiling myapp"));
        assert!(result.content.contains("Listening on"));
    }

    #[test]
    fn test_fixture_uvicorn() {
        let input = "INFO:     Started server process [12345]\n\
                     INFO:     Waiting for application startup.\n\
                     INFO:     Application startup complete.\n\
                     INFO:     Uvicorn running on http://0.0.0.0:8000\n\
                     WARNING:  StatReload detected changes in 'main.py'. Reloading...\n\
                     ERROR:    Connection error: refused";
        // Non-compact: keeps all
        let result = filter_log_output(input, false);
        assert_eq!(result.lines_filtered, 6);
        // Compact: only WARNING and ERROR
        let compact = filter_log_output(input, true);
        assert_eq!(compact.lines_filtered, 2);
        assert!(compact.content.contains("WARNING"));
        assert!(compact.content.contains("ERROR"));
    }

    #[test]
    fn test_fixture_air_go() {
        let input = "  __    _   ___\n\
                     / /\\  | | | _ \\\n\
                     /_/  \\ |_| |_| \\_\\\n\n\
                     mkdir tmp\n\
                     watching .\n\
                     watching cmd\n\
                     building...\n\
                     running...\n\
                     [GIN] 2024/01/15 - 10:00:00 | 200 |  1.234ms | ::1 | GET /api/health\n\
                     [GIN] 2024/01/15 - 10:00:00 | 200 |  1.234ms | ::1 | GET /api/health\n\
                     [GIN] 2024/01/15 - 10:00:00 | 200 |  1.234ms | ::1 | GET /api/health\n\
                     [GIN] 2024/01/15 - 10:00:01 | 500 |  5.678ms | ::1 | POST /api/users";
        let result = filter_log_output(input, false);
        // Health check lines should be deduped
        assert!(result.content.contains("(×3)"));
        assert!(result.content.contains("POST /api/users"));
    }

    #[test]
    fn test_fixture_flutter_run() {
        let input = "Launching lib/main.dart on Chrome in debug mode...\n\
                     Waiting for connection from debug service on Chrome...\n\
                     Downloading 12 crates...\n\
                     ████████████████████████████████████████ 100%\n\
                     This app is linked to the debug service\n\
                     Debug service listening on ws://127.0.0.1:12345";
        let result = filter_log_output(input, false);
        assert!(!result.content.contains("████"));
        assert!(!result.content.contains("Downloading"));
        assert!(result.content.contains("Launching"));
        assert!(result.content.contains("Debug service"));
    }

    // ── filter_log_lines helper ─────────────────────────────

    #[test]
    fn test_filter_log_lines_vec() {
        let lines: Vec<String> = vec![
            "\x1b[32m[INFO]\x1b[0m hello".into(),
            "\x1b[31m[ERROR]\x1b[0m oops".into(),
        ];
        let result = filter_log_lines(&lines, false);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "[INFO] hello");
        assert_eq!(result[1], "[ERROR] oops");
    }

    #[test]
    fn test_filter_log_lines_compact() {
        let lines: Vec<String> = vec![
            "[INFO] request ok".into(),
            "[INFO] request ok".into(),
            "[WARN] slow query".into(),
            "[ERROR] timeout".into(),
            "[INFO] request ok".into(),
        ];
        let result = filter_log_lines(&lines, true);
        assert_eq!(result.len(), 2);
        assert!(result[0].contains("WARN"));
        assert!(result[1].contains("ERROR"));
    }
}
