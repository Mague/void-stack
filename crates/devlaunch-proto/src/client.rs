use async_trait::async_trait;

use devlaunch_core::backend::ServiceBackend;
use devlaunch_core::error::{DevLaunchError, Result};
use devlaunch_core::model::ServiceState;

use crate::pb;
use crate::DevLaunchClient;

/// gRPC client that implements ServiceBackend for daemon mode.
pub struct DaemonClient {
    client: DevLaunchClient<tonic::transport::Channel>,
}

impl DaemonClient {
    /// Connect to a running daemon.
    pub async fn connect(addr: &str) -> std::result::Result<Self, tonic::transport::Error> {
        let client = DevLaunchClient::connect(addr.to_string()).await?;
        Ok(Self { client })
    }

    /// Try connecting with a timeout.
    pub async fn connect_with_timeout(
        addr: &str,
        timeout: std::time::Duration,
    ) -> std::result::Result<Self, tonic::transport::Error> {
        let endpoint = tonic::transport::Channel::from_shared(addr.to_string())
            .expect("valid URI")
            .connect_timeout(timeout);
        let channel = endpoint.connect().await?;
        let client = DevLaunchClient::new(channel);
        Ok(Self { client })
    }

    /// Ping the daemon, returns version and uptime.
    pub async fn ping(&mut self) -> Result<pb::PingResponse> {
        let resp = self
            .client
            .ping(pb::PingRequest {})
            .await
            .map_err(|e| DevLaunchError::RunnerError(e.to_string()))?;
        Ok(resp.into_inner())
    }

    /// Request daemon shutdown.
    pub async fn shutdown(&mut self) -> Result<()> {
        self.client
            .shutdown(pb::ShutdownRequest {})
            .await
            .map_err(|e| DevLaunchError::RunnerError(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ServiceBackend for DaemonClient {
    async fn start_all(&self) -> Result<Vec<ServiceState>> {
        let mut client = self.client.clone();
        let resp = client
            .start_all(pb::StartAllRequest {})
            .await
            .map_err(|e| DevLaunchError::RunnerError(e.to_string()))?;

        Ok(resp
            .into_inner()
            .states
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn start_one(&self, name: &str) -> Result<ServiceState> {
        let mut client = self.client.clone();
        let resp = client
            .start_one(pb::StartOneRequest {
                service_name: name.to_string(),
            })
            .await
            .map_err(|e| match e.code() {
                tonic::Code::NotFound => DevLaunchError::ServiceNotFound {
                    project: String::new(),
                    service: name.to_string(),
                },
                _ => DevLaunchError::RunnerError(e.to_string()),
            })?;

        let state = resp
            .into_inner()
            .state
            .ok_or_else(|| DevLaunchError::RunnerError("Empty response".into()))?;
        Ok(state.into())
    }

    async fn stop_all(&self) -> Result<()> {
        let mut client = self.client.clone();
        client
            .stop_all(pb::StopAllRequest {})
            .await
            .map_err(|e| DevLaunchError::RunnerError(e.to_string()))?;
        Ok(())
    }

    async fn stop_one(&self, name: &str) -> Result<()> {
        let mut client = self.client.clone();
        client
            .stop_one(pb::StopOneRequest {
                service_name: name.to_string(),
            })
            .await
            .map_err(|e| match e.code() {
                tonic::Code::NotFound => DevLaunchError::ServiceNotFound {
                    project: String::new(),
                    service: name.to_string(),
                },
                _ => DevLaunchError::RunnerError(e.to_string()),
            })?;
        Ok(())
    }

    async fn get_states(&self) -> Result<Vec<ServiceState>> {
        let mut client = self.client.clone();
        let resp = client
            .get_states(pb::GetStatesRequest {})
            .await
            .map_err(|e| DevLaunchError::RunnerError(e.to_string()))?;

        Ok(resp
            .into_inner()
            .states
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn get_state(&self, name: &str) -> Result<Option<ServiceState>> {
        let mut client = self.client.clone();
        let resp = client
            .get_state(pb::GetStateRequest {
                service_name: name.to_string(),
            })
            .await;

        match resp {
            Ok(r) => Ok(r.into_inner().state.map(Into::into)),
            Err(e) if e.code() == tonic::Code::NotFound => Ok(None),
            Err(e) => Err(DevLaunchError::RunnerError(e.to_string())),
        }
    }

    async fn refresh_status(&self) -> Result<()> {
        let mut client = self.client.clone();
        client
            .refresh_status(pb::RefreshStatusRequest {})
            .await
            .map_err(|e| DevLaunchError::RunnerError(e.to_string()))?;
        Ok(())
    }
}
