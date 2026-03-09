use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::model::ServiceState;
use super::url::detect_url;

/// Max log lines kept per service.
pub(crate) const MAX_LOG_LINES: usize = 5000;

/// Spawn a background task that reads lines from a child's stdout/stderr,
/// stores them in logs, detects URLs, and updates last_log_line.
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
        let name2 = name.clone();
        tokio::spawn(async move {
            info!(service = %name2, "Log reader started (stderr)");
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                process_log_line(&name2, &line, &states, &logs).await;
            }
            info!(service = %name2, "Log reader ended (stderr)");
        });
    }
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
            if let Some(url) = detect_url(line) {
                if state.url.as_deref() != Some(&url) {
                    info!(service = %service_name, url = %url, "Detected service URL");
                    state.url = Some(url);
                }
            }
        }
    }
}
