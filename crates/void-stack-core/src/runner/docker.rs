//! Docker runner — executes services inside Docker containers.
//!
//! Handles three modes:
//! 1. **Raw docker command** — command starts with "docker" → run as-is
//! 2. **Docker image** — command looks like `image:tag` → `docker run`
//! 3. **Dockerfile build** — working_dir has a Dockerfile → build + run

use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::{info, warn};

use crate::error::{Result, VoidStackError};
use crate::model::{Service, ServiceState, ServiceStatus, Target};
use crate::process_util::HideWindow;
use super::{Runner, StartResult};

/// Runs services inside Docker containers.
pub struct DockerRunner;

impl DockerRunner {
    pub fn new() -> Self {
        Self
    }

    /// Derive a stable container name from the service name.
    /// Format: `vs-<service-name>` (lowercase, replace non-alnum with dash).
    fn container_name(service: &Service) -> String {
        let clean: String = service
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        format!("vs-{}", clean.to_lowercase())
    }

    /// Detect which mode to use for this service.
    fn detect_mode(service: &Service, working_dir: &str) -> DockerMode {
        let cmd = service.command.trim();

        // Mode 1: raw docker command
        if cmd.starts_with("docker ") {
            return DockerMode::Raw;
        }

        // Mode 2: looks like a docker image reference (e.g., postgres:16, nginx, redis:7-alpine)
        if looks_like_image(cmd) {
            return DockerMode::Image(cmd.to_string());
        }

        // Mode 3: working dir has a Dockerfile → build + run
        let dir = Path::new(working_dir);
        if dir.join("Dockerfile").exists() || dir.join("dockerfile").exists() {
            return DockerMode::Build;
        }

        // Fallback: treat command as raw docker run
        DockerMode::Raw
    }

    /// Build the docker command for each mode.
    fn build_command(service: &Service, project_path: &str) -> Command {
        let working_dir = service
            .working_dir
            .as_deref()
            .unwrap_or(project_path);
        let working_dir = super::local::strip_win_prefix(working_dir);
        let container = Self::container_name(service);

        match Self::detect_mode(service, &working_dir) {
            DockerMode::Raw => {
                // Run the user's docker command as-is via cmd /c
                let mut cmd = Command::new("cmd");
                cmd.args(["/c", &service.command]);
                cmd.current_dir(&working_dir);
                cmd
            }
            DockerMode::Image(image) => {
                // docker run --name <container> --rm <env_vars> <ports> <image>
                let mut cmd = Command::new("docker");
                let mut args: Vec<String> = vec![
                    "run".into(),
                    "--name".into(),
                    container,
                    "--rm".into(),
                ];

                // Add env vars
                for (key, value) in &service.env_vars {
                    args.push("-e".into());
                    args.push(format!("{}={}", key, value));
                }

                // Add docker config if present
                if let Some(ref docker) = service.docker {
                    for port in &docker.ports {
                        args.push("-p".into());
                        args.push(port.clone());
                    }
                    for vol in &docker.volumes {
                        args.push("-v".into());
                        args.push(vol.clone());
                    }
                    for arg in &docker.extra_args {
                        args.push(arg.clone());
                    }
                }

                args.push(image);

                let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                cmd.args(&str_args);
                cmd.current_dir(&working_dir);
                cmd
            }
            DockerMode::Build => {
                // Build image from Dockerfile, then run with the service command
                // We chain: docker build -t <tag> . && docker run --name <container> --rm <tag> <cmd>
                let tag = format!("vs-build-{}", service.name.to_lowercase());

                let mut env_args = String::new();
                for (key, value) in &service.env_vars {
                    env_args.push_str(&format!(" -e {}={}", key, value));
                }

                let mut docker_args = String::new();
                if let Some(ref docker) = service.docker {
                    for port in &docker.ports {
                        docker_args.push_str(&format!(" -p {}", port));
                    }
                    for vol in &docker.volumes {
                        docker_args.push_str(&format!(" -v {}", vol));
                    }
                    for arg in &docker.extra_args {
                        docker_args.push_str(&format!(" {}", arg));
                    }
                }

                let run_cmd = format!(
                    "docker build -t {} . && docker run --name {} --rm{}{} {} {}",
                    tag, container, env_args, docker_args, tag, service.command
                );

                let mut cmd = Command::new("cmd");
                cmd.args(["/c", &run_cmd]);
                cmd.current_dir(&working_dir);
                cmd
            }
        }
    }

    /// Stop a container by name using `docker stop`.
    async fn stop_container(container: &str) -> Result<()> {
        let mut cmd = Command::new("docker");
        cmd.args(["stop", "-t", "10", container]);
        cmd.hide_window();
        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If container not found, it's already stopped — not an error
            if !stderr.contains("No such container") {
                warn!(container = %container, error = %stderr, "docker stop failed");
            }
        }
        Ok(())
    }

    /// Check if a container is running via `docker inspect`.
    pub async fn is_container_running(container: &str) -> bool {
        let mut cmd = Command::new("docker");
        cmd.args(["inspect", "--format", "{{.State.Running}}", container]);
        cmd.hide_window();

        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.trim() == "true"
            }
            Err(_) => false,
        }
    }
}

#[async_trait]
impl Runner for DockerRunner {
    fn target(&self) -> Target {
        Target::Docker
    }

    async fn start(&self, service: &Service, project_path: &str) -> Result<StartResult> {
        info!(
            service = %service.name,
            command = %service.command,
            "Starting Docker service"
        );

        let container = Self::container_name(service);

        // Clean up any existing stopped container with the same name
        let mut rm_cmd = Command::new("docker");
        rm_cmd.args(["rm", "-f", &container]);
        rm_cmd.hide_window();
        let _ = rm_cmd.output().await; // ignore errors

        let mut cmd = Self::build_command(service, project_path);

        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.hide_window();

        let child = cmd.spawn().map_err(|e| {
            VoidStackError::ProcessStartFailed(format!(
                "Docker: {} ({}): {}",
                service.name, service.command, e
            ))
        })?;

        let pid = child.id().unwrap_or(0);
        info!(service = %service.name, pid = pid, container = %container, "Docker service started");

        let mut state = ServiceState::new(service.name.clone());
        state.status = ServiceStatus::Running;
        state.pid = Some(pid);
        state.started_at = Some(chrono::Utc::now());

        Ok(StartResult { state, child })
    }

    async fn stop(&self, service: &Service, pid: u32) -> Result<()> {
        info!(service = %service.name, pid = pid, "Stopping Docker service");

        let container = Self::container_name(service);

        // Stop the container first (graceful shutdown)
        Self::stop_container(&container).await?;

        // Also kill the docker process if still alive (the `docker run` process)
        #[cfg(target_os = "windows")]
        {
            let mut kill_cmd = Command::new("taskkill");
            kill_cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
            kill_cmd.hide_window();
            let _ = kill_cmd.output().await;
        }

        Ok(())
    }

    async fn is_running(&self, pid: u32) -> Result<bool> {
        // First check if the docker process is alive
        #[cfg(target_os = "windows")]
        {
            let mut list_cmd = Command::new("tasklist");
            list_cmd.args(["/FI", &format!("PID eq {}", pid), "/NH"]);
            list_cmd.hide_window();
            let output = list_cmd.output().await?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.contains(&pid.to_string()))
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok(std::path::Path::new(&format!("/proc/{}", pid)).exists())
        }
    }
}

/// Docker execution mode.
enum DockerMode {
    /// Command starts with "docker" — run as-is
    Raw,
    /// Command is a docker image reference — `docker run <image>`
    Image(String),
    /// Working dir has a Dockerfile — build + run
    Build,
}

/// Check if a string looks like a Docker image reference.
/// Examples: `postgres:16`, `redis`, `nginx:alpine`, `myregistry.io/app:v2`
fn looks_like_image(s: &str) -> bool {
    let s = s.trim();

    // Must not contain spaces (a command would have args)
    if s.contains(' ') {
        return false;
    }

    // Must not start with common command prefixes
    let non_images = ["npm", "python", "node", "cargo", "go", "pip", "uvicorn",
                       "flask", "gunicorn", "java", "mvn", "gradle", "dotnet",
                       "ruby", "php", "dart", "flutter"];
    let lower = s.to_lowercase();
    if non_images.iter().any(|p| lower == *p || lower.starts_with(&format!("{}.", p))) {
        return false;
    }

    // Image patterns: name, name:tag, registry/name:tag
    // Must contain only valid chars: alphanumeric, -, _, ., /, :
    s.chars().all(|c| c.is_alphanumeric() || "-./_:@".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Target;

    fn docker_service(cmd: &str) -> Service {
        Service {
            name: "test-docker".to_string(),
            command: cmd.to_string(),
            target: Target::Docker,
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
            docker: None,
        }
    }

    #[test]
    fn test_container_name() {
        let svc = docker_service("test");
        assert_eq!(DockerRunner::container_name(&svc), "vs-test-docker");
    }

    #[test]
    fn test_container_name_special_chars() {
        let mut svc = docker_service("test");
        svc.name = "my app.v2".to_string();
        assert_eq!(DockerRunner::container_name(&svc), "vs-my-app-v2");
    }

    #[test]
    fn test_looks_like_image() {
        assert!(looks_like_image("postgres:16"));
        assert!(looks_like_image("redis"));
        assert!(looks_like_image("nginx:alpine"));
        assert!(looks_like_image("myregistry.io/app:v2"));
        assert!(looks_like_image("ghcr.io/org/image:latest"));

        assert!(!looks_like_image("npm run dev"));
        assert!(!looks_like_image("python main.py"));
        assert!(!looks_like_image("docker compose up"));
        assert!(!looks_like_image("python"));
        assert!(!looks_like_image("node"));
    }

    #[test]
    fn test_detect_mode_raw() {
        let svc = docker_service("docker compose up postgres");
        assert!(matches!(
            DockerRunner::detect_mode(&svc, "."),
            DockerMode::Raw
        ));
    }

    #[test]
    fn test_detect_mode_image() {
        let svc = docker_service("postgres:16");
        assert!(matches!(
            DockerRunner::detect_mode(&svc, "."),
            DockerMode::Image(_)
        ));
    }
}
