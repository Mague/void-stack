pub mod analysis;
pub mod board;
pub mod briefing;
pub mod context;
pub mod debt;
pub mod diagrams;
pub mod docker;
pub mod docs;
pub mod doctor;
#[cfg(feature = "structural")]
pub mod graph;
pub mod handoff;
pub mod orchestration;
pub mod projects;
pub mod review;
pub mod search;
pub mod services;
pub mod setup;
pub mod space;
pub mod stats;
pub mod suggest;
pub mod suppressions;

use rmcp::ErrorData as McpError;

/// Format bytes into human-readable size.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// List documentation files in a project directory.
pub fn list_doc_files(root: &str) -> Vec<String> {
    let path = std::path::Path::new(root);
    let doc_extensions = ["md", "txt"];
    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(ext) = std::path::Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                && doc_extensions.contains(&ext)
            {
                files.push(format!("  - {}", name));
            }
        }
    }
    files.sort();
    files
}

/// Helper to serialize to pretty JSON or return an MCP internal error.
pub fn to_json_pretty<T: serde::Serialize>(value: &T) -> Result<String, McpError> {
    serde_json::to_string_pretty(value).map_err(|e| McpError::internal_error(e.to_string(), None))
}

/// Test-only: point `VOID_STACK_DATA_DIR` at one shared per-process tempdir
/// so fixtures never write into the user's real data dir (config, stats,
/// indexes). Same idiom as void-stack-core's `isolate_test_data_dir`;
/// repeated calls converge on the same directory, so parallel tests in this
/// binary don't race on the env var.
#[cfg(test)]
pub(crate) fn isolate_test_data_dir() {
    use std::sync::OnceLock;
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    let dir = DIR.get_or_init(|| tempfile::tempdir().expect("tempdir for test data"));
    // SAFETY: every caller sets the same value, so races are benign.
    unsafe { std::env::set_var("VOID_STACK_DATA_DIR", dir.path()) };
}

/// Test-only: serialize tests that mutate the shared, isolated global config
/// (`config.toml`). Every config-mutating test writes to the same isolated
/// `config.toml`, so without this lock two tests can clobber each other's
/// registry. Hold the guard for the whole save→use→assert window. Uses a
/// tokio mutex so the guard can be held across `.await` points (all callers
/// are async), and it never poisons on a failed test.
#[cfg(test)]
pub(crate) async fn config_test_guard() -> tokio::sync::MutexGuard<'static, ()> {
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1_048_576), "1.0 MB");
        assert_eq!(format_size(1_572_864), "1.5 MB");
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn test_list_doc_files_filters_and_sorts() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("zeta.md"), "z").unwrap();
        std::fs::write(tmp.path().join("alpha.txt"), "a").unwrap();
        std::fs::write(tmp.path().join("code.rs"), "fn x() {}").unwrap();
        std::fs::write(tmp.path().join("data.json"), "{}").unwrap();

        let files = list_doc_files(&tmp.path().to_string_lossy());
        assert_eq!(files, vec!["  - alpha.txt", "  - zeta.md"]);
    }

    #[test]
    fn test_list_doc_files_missing_dir_is_empty() {
        assert!(list_doc_files("Z:\\no\\such\\dir\\anywhere").is_empty());
    }

    #[test]
    fn test_to_json_pretty_serializes() {
        #[derive(serde::Serialize)]
        struct S {
            a: u32,
        }
        let out = to_json_pretty(&S { a: 7 }).unwrap();
        assert!(out.contains("\"a\": 7"));
    }
}
