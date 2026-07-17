use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::url::detect_url;
use crate::model::ServiceState;

/// Max log lines kept per service.
pub(crate) const MAX_LOG_LINES: usize = 5000;

/// Spawn background tasks that read lines from a child's stdout/stderr,
/// store them in logs, detect URLs, update last_log_line, and watch for
/// process exit to mark the service as Failed/Stopped.
///
/// Takes ownership of the Child handle for efficient exit watching via
/// `child.wait()` instead of PID polling.
///
/// Returns a watch receiver that flips to `true` the moment the process
/// exits (or the handle becomes unusable), so stop paths can await actual
/// termination instead of sleeping and re-polling PIDs.
pub(crate) fn spawn_log_reader(
    service_name: String,
    mut child: Child,
    states: Arc<Mutex<HashMap<String, ServiceState>>>,
    logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
) -> tokio::sync::watch::Receiver<bool> {
    let (exit_tx, exit_rx) = tokio::sync::watch::channel(false);
    // Take stdout and stderr from child
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let name = service_name.clone();
    if let Some(stdout) = stdout {
        let states = Arc::clone(&states);
        let logs = Arc::clone(&logs);
        let name = name.clone();
        tokio::spawn(async move {
            info!(service = %name, "Log reader started (stdout)");
            read_lines_batched(stdout, &name, &states, &logs).await;
            info!(service = %name, "Log reader ended (stdout)");
        });
    }

    if let Some(stderr) = stderr {
        let states_err = Arc::clone(&states);
        let logs_err = Arc::clone(&logs);
        let name2 = name.clone();
        tokio::spawn(async move {
            info!(service = %name2, "Log reader started (stderr)");
            read_lines_batched(stderr, &name2, &states_err, &logs_err).await;
            info!(service = %name2, "Log reader ended (stderr)");
        });
    }

    // Watch for process exit using child.wait() — efficient, no polling
    let exit_states = Arc::clone(&states);
    let exit_logs = Arc::clone(&logs);
    let exit_name = service_name;
    tokio::spawn(async move {
        let wait_result = child.wait().await;
        // Notify stop paths immediately, before any state bookkeeping.
        let _ = exit_tx.send(true);
        match wait_result {
            Ok(status) => {
                // Check if service was already marked as Stopped (intentional stop)
                let current_status = {
                    let states = exit_states.lock().await;
                    states.get(&exit_name).map(|s| s.status)
                };

                if current_status == Some(crate::model::ServiceStatus::Stopped) {
                    return;
                }

                let failed = !status.success();
                let msg = if failed {
                    format!(
                        "[void-stack] Process exited with code {}",
                        status.code().unwrap_or(-1)
                    )
                } else {
                    "[void-stack] Process exited normally".to_string()
                };

                info!(service = %exit_name, ?status, "Process exited");

                let new_status = if failed {
                    crate::model::ServiceStatus::Failed
                } else {
                    crate::model::ServiceStatus::Stopped
                };

                let mut states = exit_states.lock().await;
                if let Some(state) = states.get_mut(&exit_name) {
                    state.status = new_status;
                    state.pid = None;
                    if state.last_log_line.is_none() || failed {
                        state.last_log_line = Some(msg.clone());
                    }
                }
                drop(states);

                let mut logs = exit_logs.lock().await;
                if let Some(buf) = logs.get_mut(&exit_name) {
                    buf.push(msg);
                }
            }
            Err(e) => {
                tracing::warn!(service = %exit_name, error = %e, "Error waiting for child process");
            }
        }
    });

    exit_rx
}

/// Read lines from a stream with batching: accumulates up to 64 lines
/// before flushing to the shared state, reducing lock acquisitions.
async fn read_lines_batched<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    service_name: &str,
    states: &Arc<Mutex<HashMap<String, ServiceState>>>,
    logs: &Arc<Mutex<HashMap<String, Vec<String>>>>,
) {
    const BATCH_SIZE: usize = 64;
    let reader = BufReader::new(reader);
    let mut lines = reader.lines();
    let mut batch: Vec<String> = Vec::with_capacity(BATCH_SIZE);

    loop {
        // Try to read with a short timeout to flush partial batches
        let line =
            tokio::time::timeout(std::time::Duration::from_millis(50), lines.next_line()).await;

        match line {
            Ok(Ok(Some(line))) => {
                let clean = strip_ansi(&line);
                debug!(service = %service_name, line = %clean, "Captured log line");
                batch.push(clean);
                if batch.len() >= BATCH_SIZE {
                    flush_batch(service_name, &mut batch, states, logs).await;
                }
            }
            Ok(Ok(None)) => {
                // Stream ended
                if !batch.is_empty() {
                    flush_batch(service_name, &mut batch, states, logs).await;
                }
                break;
            }
            Ok(Err(_)) => {
                // Read error — flush and stop
                if !batch.is_empty() {
                    flush_batch(service_name, &mut batch, states, logs).await;
                }
                break;
            }
            Err(_) => {
                // Timeout — flush partial batch to keep UI responsive
                if !batch.is_empty() {
                    flush_batch(service_name, &mut batch, states, logs).await;
                }
            }
        }
    }
}

/// Flush a batch of log lines: acquires both locks once for the entire batch.
async fn flush_batch(
    service_name: &str,
    batch: &mut Vec<String>,
    states: &Arc<Mutex<HashMap<String, ServiceState>>>,
    logs: &Arc<Mutex<HashMap<String, Vec<String>>>>,
) {
    if batch.is_empty() {
        return;
    }

    let last_line = batch.last().cloned();
    let mut detected_url: Option<String> = None;

    // Scan batch for URLs (check all lines, keep last match)
    for line in batch.iter() {
        if let Some(url) = detect_url(line) {
            detected_url = Some(url);
        }
    }

    // Single lock acquisition for logs
    {
        let mut logs = logs.lock().await;
        if let Some(buf) = logs.get_mut(service_name) {
            buf.append(batch);
            if buf.len() > MAX_LOG_LINES {
                let drain = buf.len() - MAX_LOG_LINES;
                buf.drain(..drain);
            }
        }
    }

    // Single lock acquisition for state
    {
        let mut states = states.lock().await;
        if let Some(state) = states.get_mut(service_name) {
            if let Some(ref line) = last_line {
                state.last_log_line = Some(line.clone());
            }
            if let Some(ref url) = detected_url
                && state.url.as_deref() != Some(url)
            {
                info!(service = %service_name, url = %url, "Detected service URL");
                state.url = Some(url.clone());
            }
        }
    }

    batch.clear();
}

/// Strip ANSI escape codes — delegates to the shared log_filter implementation.
fn strip_ansi(s: &str) -> String {
    crate::log_filter::strip_ansi(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    type States = Arc<Mutex<HashMap<String, ServiceState>>>;
    type Logs = Arc<Mutex<HashMap<String, Vec<String>>>>;

    /// Build shared state/log maps pre-seeded with one service entry.
    fn setup(service: &str) -> (States, Logs) {
        let mut state_map = HashMap::new();
        state_map.insert(service.to_string(), ServiceState::new(service.to_string()));
        let mut log_map = HashMap::new();
        log_map.insert(service.to_string(), Vec::<String>::new());
        (
            Arc::new(Mutex::new(state_map)),
            Arc::new(Mutex::new(log_map)),
        )
    }

    #[tokio::test]
    async fn test_flush_batch_appends_lines_and_updates_last_log_line() {
        let (states, logs) = setup("api");
        let mut batch = vec!["first".to_string(), "second".to_string()];

        flush_batch("api", &mut batch, &states, &logs).await;

        let logs_guard = logs.lock().await;
        assert_eq!(
            logs_guard.get("api").expect("buffer must exist"),
            &vec!["first".to_string(), "second".to_string()],
            "batch lines should be appended in order"
        );
        drop(logs_guard);

        let states_guard = states.lock().await;
        let state = states_guard.get("api").expect("state must exist");
        assert_eq!(
            state.last_log_line.as_deref(),
            Some("second"),
            "last_log_line should track the last line of the batch"
        );
        assert!(batch.is_empty(), "batch should be cleared after flushing");
    }

    #[tokio::test]
    async fn test_flush_batch_empty_batch_is_noop() {
        let (states, logs) = setup("api");
        let mut batch: Vec<String> = Vec::new();

        flush_batch("api", &mut batch, &states, &logs).await;

        assert!(
            logs.lock().await.get("api").unwrap().is_empty(),
            "empty batch must not touch the log buffer"
        );
        assert_eq!(
            states.lock().await.get("api").unwrap().last_log_line,
            None,
            "empty batch must not touch last_log_line"
        );
    }

    #[tokio::test]
    async fn test_flush_batch_rotates_buffer_beyond_max_log_lines() {
        let (states, logs) = setup("api");
        {
            let mut guard = logs.lock().await;
            let buf = guard.get_mut("api").unwrap();
            for i in 0..MAX_LOG_LINES {
                buf.push(format!("old-{}", i));
            }
        }
        let mut batch = vec![
            "new-0".to_string(),
            "new-1".to_string(),
            "new-2".to_string(),
        ];

        flush_batch("api", &mut batch, &states, &logs).await;

        let guard = logs.lock().await;
        let buf = guard.get("api").unwrap();
        assert_eq!(
            buf.len(),
            MAX_LOG_LINES,
            "buffer must be capped at MAX_LOG_LINES"
        );
        assert_eq!(buf[0], "old-3", "oldest lines should be drained first");
        assert_eq!(
            buf[buf.len() - 1],
            "new-2",
            "newest line should be at the end"
        );
    }

    #[tokio::test]
    async fn test_flush_batch_detects_last_url_in_batch() {
        let (states, logs) = setup("web");
        let mut batch = vec![
            "Ready on http://localhost:3000".to_string(),
            "Also listening on http://127.0.0.1:4000".to_string(),
        ];

        flush_batch("web", &mut batch, &states, &logs).await;

        let states_guard = states.lock().await;
        let state = states_guard.get("web").unwrap();
        assert_eq!(
            state.url.as_deref(),
            Some("http://127.0.0.1:4000"),
            "the last URL in the batch should win"
        );
    }

    #[tokio::test]
    async fn test_flush_batch_unknown_service_drops_lines_without_panic() {
        let (states, logs) = setup("known");
        let mut batch = vec!["orphan line".to_string()];

        flush_batch("ghost", &mut batch, &states, &logs).await;

        assert!(
            batch.is_empty(),
            "batch is cleared even for unknown services"
        );
        assert!(
            logs.lock().await.get("known").unwrap().is_empty(),
            "other services' buffers must not be touched"
        );
    }

    #[tokio::test]
    async fn test_read_lines_batched_flushes_on_eof_and_strips_ansi() {
        let (states, logs) = setup("web");
        // In-memory stream: &[u8] implements AsyncRead, so no process is needed.
        let input: &[u8] = b"building...\n\x1b[36mLocal: http://localhost:5173/\x1b[0m\n";

        read_lines_batched(input, "web", &states, &logs).await;

        let logs_guard = logs.lock().await;
        let buf = logs_guard.get("web").expect("buffer must exist");
        assert_eq!(buf.len(), 2, "both lines should be captured");
        assert_eq!(buf[0], "building...", "plain lines pass through unchanged");
        assert_eq!(
            buf[1], "Local: http://localhost:5173/",
            "ANSI escape codes should be stripped before storing"
        );
        drop(logs_guard);

        let states_guard = states.lock().await;
        let state = states_guard.get("web").unwrap();
        assert_eq!(
            state.last_log_line.as_deref(),
            Some("Local: http://localhost:5173/"),
            "last_log_line should be updated on flush"
        );
        assert_eq!(
            state.url.as_deref(),
            Some("http://localhost:5173/"),
            "the service URL should be detected from log output"
        );
    }

    #[test]
    fn test_strip_ansi_removes_escape_codes() {
        assert_eq!(
            strip_ansi("\x1b[32mready\x1b[0m in 300ms"),
            "ready in 300ms",
            "color codes should be removed"
        );
        assert_eq!(
            strip_ansi("no codes"),
            "no codes",
            "plain text should be unchanged"
        );
    }

    // ── spawn_log_reader (exit-watcher integration) ────────────

    use crate::model::ServiceStatus;
    use std::process::Stdio;
    use std::time::Duration;

    /// Spawn a short-lived real child process with piped stdout/stderr.
    /// `line` is echoed to stdout; `fail` makes it exit with code 3.
    fn spawn_test_child(line: &str, fail: bool) -> Child {
        let mut cmd = if cfg!(windows) {
            let mut c = tokio::process::Command::new("cmd");
            let script = if fail {
                format!("echo {} & exit 3", line)
            } else {
                format!("echo {}", line)
            };
            c.args(["/c", &script]);
            c
        } else {
            let mut c = tokio::process::Command::new("sh");
            let script = if fail {
                format!("echo {}; exit 3", line)
            } else {
                format!("echo {}", line)
            };
            c.args(["-c", &script]);
            c
        };
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.spawn().expect("failed to spawn test child")
    }

    /// Mark a seeded service as Running so the exit watcher performs its
    /// state transition (the default state is Stopped, which short-circuits).
    async fn mark_running(states: &States, name: &str) {
        let mut guard = states.lock().await;
        if let Some(state) = guard.get_mut(name) {
            state.status = ServiceStatus::Running;
            state.pid = Some(std::process::id());
        }
    }

    /// Poll the service status until it matches `want` or the retries run out.
    async fn wait_for_status(states: &States, name: &str, want: ServiceStatus) -> bool {
        for _ in 0..150 {
            {
                let guard = states.lock().await;
                if guard.get(name).map(|s| s.status) == Some(want) {
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[tokio::test]
    async fn test_spawn_log_reader_marks_stopped_on_clean_exit() {
        let (states, logs) = setup("web");
        mark_running(&states, "web").await;

        let child = spawn_test_child("Ready on http://localhost:3000", false);
        let mut exit_rx =
            spawn_log_reader("web".into(), child, Arc::clone(&states), Arc::clone(&logs));

        // The watch flips the moment the process exits.
        assert!(exit_rx.wait_for(|e| *e).await.is_ok());
        assert!(
            wait_for_status(&states, "web", ServiceStatus::Stopped).await,
            "a clean exit from a Running service must transition to Stopped"
        );

        // The URL echoed on stdout should be detected and stored.
        let states_guard = states.lock().await;
        let state = states_guard.get("web").unwrap();
        assert_eq!(state.pid, None, "pid must be cleared on exit");
        assert_eq!(state.url.as_deref(), Some("http://localhost:3000"));
        drop(states_guard);

        let logs_guard = logs.lock().await;
        let buf = logs_guard.get("web").unwrap();
        assert!(
            buf.iter().any(|l| l.contains("Process exited normally")),
            "the normal-exit marker should be appended to the log buffer"
        );
    }

    #[tokio::test]
    async fn test_spawn_log_reader_marks_failed_on_nonzero_exit() {
        let (states, logs) = setup("api");
        mark_running(&states, "api").await;

        let child = spawn_test_child("booting", true);
        let mut exit_rx =
            spawn_log_reader("api".into(), child, Arc::clone(&states), Arc::clone(&logs));

        assert!(exit_rx.wait_for(|e| *e).await.is_ok());
        assert!(
            wait_for_status(&states, "api", ServiceStatus::Failed).await,
            "a non-zero exit must transition the service to Failed"
        );

        let states_guard = states.lock().await;
        let state = states_guard.get("api").unwrap();
        assert!(
            state
                .last_log_line
                .as_deref()
                .is_some_and(|l| l.contains("exited with code")),
            "the failure marker must be surfaced in last_log_line"
        );
    }

    #[tokio::test]
    async fn test_spawn_log_reader_respects_intentional_stop() {
        // Service left in the default Stopped state: the exit watcher must
        // treat the exit as intentional and skip the exit bookkeeping.
        let (states, logs) = setup("worker");

        let child = spawn_test_child("started", false);
        let mut exit_rx = spawn_log_reader(
            "worker".into(),
            child,
            Arc::clone(&states),
            Arc::clone(&logs),
        );

        assert!(exit_rx.wait_for(|e| *e).await.is_ok());
        // Give the (early-returning) watcher task a moment to run.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let states_guard = states.lock().await;
        assert_eq!(
            states_guard.get("worker").unwrap().status,
            ServiceStatus::Stopped,
            "an intentional stop must not be flipped to Failed/Stopped-by-exit"
        );
        drop(states_guard);

        let logs_guard = logs.lock().await;
        let buf = logs_guard.get("worker").unwrap();
        assert!(
            !buf.iter().any(|l| l.contains("Process exited")),
            "no exit marker is appended when the stop was intentional"
        );
    }
}
