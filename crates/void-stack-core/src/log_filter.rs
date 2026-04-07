//! Intelligent log output filtering — reduces noise from service logs.
//!
//! Strategies applied in order:
//! 1. Strip ANSI escape codes
//! 2. Deduplicate consecutive repeated lines → "message (×N)"
//! 3. Remove progress bars and download indicators
//! 4. If compact=true: filter by log level (keep only WARN/ERROR)
//! 5. Truncate long output (first 20 + last 30, middle omitted)

use regex::Regex;

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

// ── Strip ANSI ──────────────────────────────────────────────

/// Strip ANSI escape codes from a string.
pub fn strip_ansi(s: &str) -> String {
    // Use a simple regex for ANSI CSI sequences
    let re = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
    re.replace_all(s, "").to_string()
}

// ── Progress bar detection ──────────────────────────────────

/// Returns true if the line looks like a progress bar or download indicator.
fn is_progress_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Lines that are only progress characters
    let progress_chars = ['█', '░', '▓', '▒', '─', '━', '=', '>', '<'];
    let progress_only = trimmed.chars().all(|c| {
        progress_chars.contains(&c)
            || c == '['
            || c == ']'
            || c == ' '
            || c == '%'
            || c.is_ascii_digit()
            || c == '.'
            || c == '/'
    });
    if progress_only && trimmed.len() > 3 {
        return true;
    }

    // Common progress patterns
    let progress_patterns = [
        // npm/pip download bars
        r"^\s*\[?[=>#\-]+\]?\s*\d+%",
        // Downloading X of Y
        r"(?i)^(downloading|fetching|extracting)\s+",
        // spinner patterns: ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏
        r"^[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]",
        // Cargo-style: Downloading crates ...
        r"^\s*Downloading\s+\d+\s+crates?",
        // pip-style: ━━━━━━━━
        r"━{4,}",
        // progress: 50/100 or 50.0%
        r"^\s*\d+/\d+\s*$",
        r"^\s*\d+(\.\d+)?%\s*$",
    ];

    for pat in &progress_patterns {
        if let Ok(re) = Regex::new(pat)
            && re.is_match(trimmed)
        {
            return true;
        }
    }

    false
}

// ── Deduplication ───────────────────────────────────────────

fn deduplicate(lines: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut prev: Option<&str> = None;
    let mut count: usize = 0;

    for line in lines {
        if Some(line.as_str()) == prev {
            count += 1;
        } else {
            // Flush previous
            if count > 0
                && let Some(last) = result.last_mut()
            {
                *last = format!("{} (×{})", last, count + 1);
            }
            result.push(line.clone());
            prev = Some(line.as_str());
            count = 0;
        }
    }

    // Flush final
    if count > 0
        && let Some(last) = result.last_mut()
    {
        *last = format!("{} (×{})", last, count + 1);
    }

    result
}

// ── Level filtering ─────────────────────────────────────────

/// Check if the log output contains log level markers.
fn has_log_levels(lines: &[String]) -> bool {
    let level_count = lines
        .iter()
        .filter(|l| {
            let upper = l.to_uppercase();
            upper.contains("INFO")
                || upper.contains("WARN")
                || upper.contains("ERROR")
                || upper.contains("DEBUG")
                || upper.contains("TRACE")
        })
        .count();
    // At least 20% of lines should have levels for this to be level-based output
    level_count > 0 && (level_count * 5 >= lines.len())
}

/// Returns true if the line is INFO, DEBUG, or TRACE level (low priority in compact mode).
fn is_low_priority_level(line: &str) -> bool {
    let upper = line.to_uppercase();
    // Only filter if the line actually has a level marker
    let has_level = upper.contains("INFO")
        || upper.contains("WARN")
        || upper.contains("ERROR")
        || upper.contains("DEBUG")
        || upper.contains("TRACE");

    if !has_level {
        // Lines without explicit level are kept (could be stack traces, etc.)
        return false;
    }

    // Keep WARN, ERROR, FATAL, PANIC
    if upper.contains("WARN")
        || upper.contains("ERROR")
        || upper.contains("FATAL")
        || upper.contains("PANIC")
    {
        return false;
    }

    // Filter out INFO, DEBUG, TRACE
    true
}

// ── Truncation ──────────────────────────────────────────────

const MAX_LINES: usize = 150;
const HEAD_LINES: usize = 20;
const TAIL_LINES: usize = 30;

fn truncate_lines(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= MAX_LINES {
        return lines;
    }

    let omitted = lines.len() - HEAD_LINES - TAIL_LINES;
    let mut result = Vec::with_capacity(HEAD_LINES + TAIL_LINES + 1);
    result.extend_from_slice(&lines[..HEAD_LINES]);
    result.push(format!("... [{} lines omitted] ...", omitted));
    result.extend_from_slice(&lines[lines.len() - TAIL_LINES..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ANSI stripping ──────────────────────────────────────

    #[test]
    fn test_strip_ansi_basic() {
        assert_eq!(strip_ansi("\x1b[32mhello\x1b[0m"), "hello");
    }

    #[test]
    fn test_strip_ansi_multiple() {
        let input = "\x1b[1m\x1b[31mERROR\x1b[0m: something failed";
        assert_eq!(strip_ansi(input), "ERROR: something failed");
    }

    #[test]
    fn test_strip_ansi_no_codes() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    // ── Progress bar detection ──────────────────────────────

    #[test]
    fn test_progress_bar_percent() {
        assert!(is_progress_line("  50%"));
        assert!(is_progress_line("100.0%"));
    }

    #[test]
    fn test_progress_bar_blocks() {
        assert!(is_progress_line("████████░░░░ 65%"));
    }

    #[test]
    fn test_progress_bar_equals() {
        assert!(is_progress_line("[======>   ] 60%"));
    }

    #[test]
    fn test_progress_bar_downloading() {
        assert!(is_progress_line("Downloading crate actix-web..."));
        assert!(is_progress_line("downloading packages..."));
    }

    #[test]
    fn test_progress_pip_bar() {
        assert!(is_progress_line("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"));
    }

    #[test]
    fn test_not_progress_regular_line() {
        assert!(!is_progress_line("Server started on port 3000"));
        assert!(!is_progress_line("[INFO] Starting application"));
    }

    #[test]
    fn test_not_progress_empty() {
        assert!(!is_progress_line(""));
        assert!(!is_progress_line("   "));
    }

    // ── Deduplication ───────────────────────────────────────

    #[test]
    fn test_deduplicate_consecutive() {
        let lines: Vec<String> = vec![
            "hello".into(),
            "hello".into(),
            "hello".into(),
            "world".into(),
        ];
        let result = deduplicate(&lines);
        assert_eq!(result, vec!["hello (×3)", "world"]);
    }

    #[test]
    fn test_deduplicate_no_repeats() {
        let lines: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let result = deduplicate(&lines);
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_deduplicate_at_end() {
        let lines: Vec<String> = vec!["a".into(), "b".into(), "b".into()];
        let result = deduplicate(&lines);
        assert_eq!(result, vec!["a", "b (×2)"]);
    }

    // ── Level filtering ─────────────────────────────────────

    #[test]
    fn test_has_log_levels_true() {
        let lines: Vec<String> = vec![
            "[INFO] Starting".into(),
            "[WARN] Slow query".into(),
            "[ERROR] Failed".into(),
        ];
        assert!(has_log_levels(&lines));
    }

    #[test]
    fn test_has_log_levels_false() {
        let lines: Vec<String> = vec!["plain text".into(), "more text".into()];
        assert!(!has_log_levels(&lines));
    }

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

    // ── Truncation ──────────────────────────────────────────

    #[test]
    fn test_truncate_short() {
        let lines: Vec<String> = (0..50).map(|i| format!("line {}", i)).collect();
        let result = truncate_lines(lines.clone());
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn test_truncate_long() {
        let lines: Vec<String> = (0..200).map(|i| format!("line {}", i)).collect();
        let result = truncate_lines(lines);
        // 20 head + 1 omitted marker + 30 tail = 51
        assert_eq!(result.len(), 51);
        assert!(result[0].contains("line 0"));
        assert!(result[19].contains("line 19"));
        assert!(result[20].contains("lines omitted"));
        assert!(result[21].contains("line 170"));
        assert!(result[50].contains("line 199"));
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
