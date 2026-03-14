pub mod pb {
    tonic::include_proto!("void_stack");
}

pub mod client;

// Re-export for convenience
pub use pb::void_stack_client::VoidStackClient;
pub use pb::void_stack_server::{VoidStack, VoidStackServer};

// --- Conversions between core types and protobuf types ---

use void_stack_core::model::{ServiceState as CoreState, ServiceStatus as CoreStatus};

impl From<CoreStatus> for pb::ServiceStatus {
    fn from(s: CoreStatus) -> Self {
        match s {
            CoreStatus::Stopped => pb::ServiceStatus::Stopped,
            CoreStatus::Starting => pb::ServiceStatus::Starting,
            CoreStatus::Running => pb::ServiceStatus::Running,
            CoreStatus::Failed => pb::ServiceStatus::Failed,
            CoreStatus::Stopping => pb::ServiceStatus::Stopping,
        }
    }
}

impl From<pb::ServiceStatus> for CoreStatus {
    fn from(s: pb::ServiceStatus) -> Self {
        match s {
            pb::ServiceStatus::Stopped => CoreStatus::Stopped,
            pb::ServiceStatus::Starting => CoreStatus::Starting,
            pb::ServiceStatus::Running => CoreStatus::Running,
            pb::ServiceStatus::Failed => CoreStatus::Failed,
            pb::ServiceStatus::Stopping => CoreStatus::Stopping,
        }
    }
}

impl From<CoreState> for pb::ServiceState {
    fn from(s: CoreState) -> Self {
        pb::ServiceState {
            service_name: s.service_name,
            status: pb::ServiceStatus::from(s.status).into(),
            pid: s.pid.unwrap_or(0),
            started_at: s.started_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            cpu_percent: s.cpu_percent.unwrap_or(0.0),
            memory_mb: s.memory_mb.unwrap_or(0.0),
            last_log_line: s.last_log_line.unwrap_or_default(),
            exit_code: s.exit_code.unwrap_or(0),
            url: s.url.unwrap_or_default(),
        }
    }
}

impl From<pb::ServiceState> for CoreState {
    fn from(s: pb::ServiceState) -> Self {
        let status = pb::ServiceStatus::try_from(s.status).unwrap_or(pb::ServiceStatus::Stopped);

        CoreState {
            service_name: s.service_name,
            status: CoreStatus::from(status),
            pid: if s.pid > 0 { Some(s.pid) } else { None },
            started_at: if s.started_at.is_empty() {
                None
            } else {
                chrono::DateTime::parse_from_rfc3339(&s.started_at)
                    .ok()
                    .map(|dt| dt.with_timezone(&chrono::Utc))
            },
            cpu_percent: if s.cpu_percent > 0.0 {
                Some(s.cpu_percent)
            } else {
                None
            },
            memory_mb: if s.memory_mb > 0.0 {
                Some(s.memory_mb)
            } else {
                None
            },
            last_log_line: if s.last_log_line.is_empty() {
                None
            } else {
                Some(s.last_log_line)
            },
            exit_code: if s.exit_code != 0 {
                Some(s.exit_code)
            } else {
                None
            },
            url: if s.url.is_empty() { None } else { Some(s.url) },
        }
    }
}
