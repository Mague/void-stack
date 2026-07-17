//! Line-level filtering rules: ANSI stripping, progress-bar detection and
//! log-level classification.

use std::sync::OnceLock;

use regex::Regex;

fn ansi_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap())
}

fn progress_regexes() -> &'static [Regex] {
    static REGEXES: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEXES.get_or_init(|| {
        [
            r"^\s*\[?[=>#\-]+\]?\s*\d+%",
            r"(?i)^(downloading|fetching|extracting)\s+",
            r"^[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]",
            r"^\s*Downloading\s+\d+\s+crates?",
            r"━{4,}",
            r"^\s*\d+/\d+\s*$",
            r"^\s*\d+(\.\d+)?%\s*$",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("static spinner/progress regexes compile"))
        .collect()
    })
}

// ── Strip ANSI ──────────────────────────────────────────────

/// Strip ANSI escape codes from a string.
pub fn strip_ansi(s: &str) -> String {
    ansi_regex().replace_all(s, "").to_string()
}

// ── Progress bar detection ──────────────────────────────────

/// Returns true if the line looks like a progress bar or download indicator.
pub(super) fn is_progress_line(line: &str) -> bool {
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

    // Common progress patterns (compiled once via OnceLock)
    for re in progress_regexes() {
        if re.is_match(trimmed) {
            return true;
        }
    }

    false
}

// ── Level filtering ─────────────────────────────────────────

/// Check if the log output contains log level markers.
pub(super) fn has_log_levels(lines: &[String]) -> bool {
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
pub(super) fn is_low_priority_level(line: &str) -> bool {
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
}
