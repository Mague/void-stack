use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::broadcast;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::info;

use void_stack_core::backend::ServiceBackend;
use void_stack_core::manager::ProcessManager;
use void_stack_proto::pb;

/// gRPC service implementation wrapping a ProcessManager.
pub struct VoidStackService {
    manager: Arc<ProcessManager>,
    started_at: Instant,
    log_tx: broadcast::Sender<pb::LogEntry>,
}

impl VoidStackService {
    pub fn new(manager: Arc<ProcessManager>, log_tx: broadcast::Sender<pb::LogEntry>) -> Self {
        Self {
            manager,
            started_at: Instant::now(),
            log_tx,
        }
    }
}

#[tonic::async_trait]
impl pb::void_stack_server::VoidStack for VoidStackService {
    async fn start_all(
        &self,
        _request: Request<pb::StartAllRequest>,
    ) -> Result<Response<pb::StartAllResponse>, Status> {
        info!("gRPC: StartAll");
        let states = ServiceBackend::start_all(self.manager.as_ref())
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let proto_states: Vec<pb::ServiceState> = states.into_iter().map(Into::into).collect();
        Ok(Response::new(pb::StartAllResponse {
            states: proto_states,
        }))
    }

    async fn start_one(
        &self,
        request: Request<pb::StartOneRequest>,
    ) -> Result<Response<pb::ServiceStateResponse>, Status> {
        let name = &request.get_ref().service_name;
        info!(service = %name, "gRPC: StartOne");

        let state = ServiceBackend::start_one(self.manager.as_ref(), name)
            .await
            .map_err(|e| match &e {
                void_stack_core::error::VoidStackError::ServiceNotFound { .. } => {
                    Status::not_found(e.to_string())
                }
                _ => Status::internal(e.to_string()),
            })?;

        Ok(Response::new(pb::ServiceStateResponse {
            state: Some(state.into()),
        }))
    }

    async fn stop_all(
        &self,
        _request: Request<pb::StopAllRequest>,
    ) -> Result<Response<pb::StopAllResponse>, Status> {
        info!("gRPC: StopAll");
        ServiceBackend::stop_all(self.manager.as_ref())
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(pb::StopAllResponse {
            success: true,
            message: "All services stopped".into(),
        }))
    }

    async fn stop_one(
        &self,
        request: Request<pb::StopOneRequest>,
    ) -> Result<Response<pb::StopOneResponse>, Status> {
        let name = &request.get_ref().service_name;
        info!(service = %name, "gRPC: StopOne");

        ServiceBackend::stop_one(self.manager.as_ref(), name)
            .await
            .map_err(|e| match &e {
                void_stack_core::error::VoidStackError::ServiceNotFound { .. } => {
                    Status::not_found(e.to_string())
                }
                _ => Status::internal(e.to_string()),
            })?;

        Ok(Response::new(pb::StopOneResponse {
            success: true,
            message: format!("Service '{name}' stopped"),
        }))
    }

    async fn get_states(
        &self,
        _request: Request<pb::GetStatesRequest>,
    ) -> Result<Response<pb::GetStatesResponse>, Status> {
        let states = ServiceBackend::get_states(self.manager.as_ref())
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let proto_states: Vec<pb::ServiceState> = states.into_iter().map(Into::into).collect();
        Ok(Response::new(pb::GetStatesResponse {
            states: proto_states,
        }))
    }

    async fn get_state(
        &self,
        request: Request<pb::GetStateRequest>,
    ) -> Result<Response<pb::ServiceStateResponse>, Status> {
        let name = &request.get_ref().service_name;
        let state = ServiceBackend::get_state(self.manager.as_ref(), name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        match state {
            Some(s) => Ok(Response::new(pb::ServiceStateResponse {
                state: Some(s.into()),
            })),
            None => Err(Status::not_found(format!("Service '{name}' not found"))),
        }
    }

    async fn refresh_status(
        &self,
        _request: Request<pb::RefreshStatusRequest>,
    ) -> Result<Response<pb::RefreshStatusResponse>, Status> {
        ServiceBackend::refresh_status(self.manager.as_ref())
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(pb::RefreshStatusResponse { success: true }))
    }

    type StreamLogsStream =
        Pin<Box<dyn Stream<Item = Result<pb::LogEntry, Status>> + Send>>;

    async fn stream_logs(
        &self,
        request: Request<pb::StreamLogsRequest>,
    ) -> Result<Response<Self::StreamLogsStream>, Status> {
        let filter_service = request.get_ref().service_name.clone();
        info!(service = %filter_service, "gRPC: StreamLogs");

        let mut rx = self.log_tx.subscribe();
        let (tx, mpsc_rx) = tokio::sync::mpsc::channel(128);

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(entry) => {
                        if filter_service.is_empty()
                            || entry.service_name == filter_service
                        {
                            if tx.send(Ok(entry)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(mpsc_rx);
        Ok(Response::new(Box::pin(stream)))
    }

    async fn shutdown(
        &self,
        _request: Request<pb::ShutdownRequest>,
    ) -> Result<Response<pb::ShutdownResponse>, Status> {
        info!("gRPC: Shutdown requested");
        // Stop all services first
        let _ = ServiceBackend::stop_all(self.manager.as_ref()).await;

        // Signal the main loop to exit (via a separate channel would be ideal,
        // but for now we just stop services and let the caller handle process exit)
        Ok(Response::new(pb::ShutdownResponse { success: true }))
    }

    async fn ping(
        &self,
        _request: Request<pb::PingRequest>,
    ) -> Result<Response<pb::PingResponse>, Status> {
        let project = self.manager.project();
        let states = ServiceBackend::get_states(self.manager.as_ref())
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let running = states
            .iter()
            .filter(|s| s.status == void_stack_core::model::ServiceStatus::Running)
            .count() as u32;

        Ok(Response::new(pb::PingResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_secs: self.started_at.elapsed().as_secs(),
            project_name: project.name.clone(),
            services_running: running,
            services_total: states.len() as u32,
        }))
    }
}
