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
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                process_log_line(&name, &line, &states, &logs).await;
            }
            info!(service = %name, "Log reader ended (stdout)");
        });
    }

    if let Some(stderr) = stderr {
        let states_err = Arc::clone(&states);
        let logs_err = Arc::clone(&logs);
        let name2 = name.clone();
        tokio::spawn(async move {
            info!(service = %name2, "Log reader started (stderr)");
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                process_log_line(&name2, &line, &states_err, &logs_err).await;
            }
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

/// Process a single log line: store it, detect URLs, update state.
async fn process_log_line(
    service_name: &str,
    line: &str,
    states: &Arc<Mutex<HashMap<String, ServiceState>>>,
    logs: &Arc<Mutex<HashMap<String, Vec<String>>>>,
) {
    debug!(service = %service_name, line = %line, "Captured log line");

    // Store in log buffer
    {
        let mut logs = logs.lock().await;
        if let Some(buf) = logs.get_mut(service_name) {
            buf.push(line.to_string());
            // Trim if too many lines
            if buf.len() > MAX_LOG_LINES {
                let drain = buf.len() - MAX_LOG_LINES;
                buf.drain(..drain);
            }
        }
    }

    // Update last_log_line and detect URLs
    {
        let mut states = states.lock().await;
        if let Some(state) = states.get_mut(service_name) {
            state.last_log_line = Some(line.to_string());

            // Detect URL -- always update to handle port fallback (e.g., Vite 3000 -> 3001)
            if let Some(url) = detect_url(line)
                && state.url.as_deref() != Some(&url)
            {
                info!(service = %service_name, url = %url, "Detected service URL");
                state.url = Some(url);
            }
        }
    }
}
