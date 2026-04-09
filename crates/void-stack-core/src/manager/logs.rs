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
pub(crate) fn spawn_log_reader(
    service_name: String,
    child: &mut Child,
    states: Arc<Mutex<HashMap<String, ServiceState>>>,
    logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
) {
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

    // Watch for process exit — update state to Failed if it dies unexpectedly
    let exit_states = Arc::clone(&states);
    let exit_logs = Arc::clone(&logs);
    let exit_name = service_name;
    let pid = child.id();
    tokio::spawn(async move {
        // Give the process a moment to start before watching
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Poll every 2s to check if the process is still alive
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            let current_status = {
                let states = exit_states.lock().await;
                states.get(&exit_name).map(|s| s.status)
            };

            match current_status {
                Some(crate::model::ServiceStatus::Running) => {
                    // Check if PID is still alive
                    let alive = if let Some(pid) = pid {
                        is_pid_alive(pid).await
                    } else {
                        false
                    };

                    if !alive {
                        info!(service = %exit_name, "Process exited unexpectedly — marking as Failed");
                        let mut states = exit_states.lock().await;
                        if let Some(state) = states.get_mut(&exit_name) {
                            state.status = crate::model::ServiceStatus::Failed;
                            state.pid = None;
                            if state.last_log_line.is_none() {
                                state.last_log_line =
                                    Some("Process exited unexpectedly".to_string());
                            }
                        }
                        // Add error to logs
                        let mut logs = exit_logs.lock().await;
                        if let Some(buf) = logs.get_mut(&exit_name) {
                            buf.push("[void-stack] Process exited unexpectedly".to_string());
                        }
                        break;
                    }
                }
                Some(crate::model::ServiceStatus::Stopped) | None => break,
                _ => {} // STARTING, STOPPING, FAILED — keep watching briefly
            }
        }
    });
}

/// Check if a PID is still alive.
async fn is_pid_alive(pid: u32) -> bool {
    crate::process_util::is_pid_alive_async(pid).await
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

/// Strip ANSI escape codes from a string.
/// Removes sequences like `\x1b[32m`, `\x1b[1m`, `\x1b[0m`, etc.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip the escape sequence: ESC [ ... (letter)
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Consume until we hit a letter (the terminator)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}
