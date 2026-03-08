use std::path::Path;

use async_trait::async_trait;

use super::{run_cmd_any, CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct CudaDetector;

#[async_trait]
impl DependencyDetector for CudaDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Cuda
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        // Check if any Python file mentions torch/cuda, or requirements.txt has torch
        let req_path = project_path.join("requirements.txt");
        if let Ok(content) = std::fs::read_to_string(&req_path) {
            if content.contains("torch") || content.contains("cuda") || content.contains("tensorflow") {
                return true;
            }
        }
        let pyproject = project_path.join("pyproject.toml");
        if let Ok(content) = std::fs::read_to_string(&pyproject) {
            if content.contains("torch") || content.contains("cuda") || content.contains("tensorflow") {
                return true;
            }
        }
        false
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Cuda);

        // Check nvidia-smi
        let smi_output = run_cmd_any("nvidia-smi", &["--query-gpu=driver_version,name,memory.total", "--format=csv,noheader"]).await;
        match smi_output {
            Some(out) if !out.is_empty() => {
                // Parse "560.94, NVIDIA GeForce RTX 4070, 12282 MiB"
                let parts: Vec<&str> = out.lines().next().unwrap_or("").split(", ").collect();
                if let Some(driver) = parts.first() {
                    status.details.push(format!("Driver: {}", driver.trim()));
                }
                if let Some(gpu) = parts.get(1) {
                    status.details.push(format!("GPU: {}", gpu.trim()));
                }
                if let Some(mem) = parts.get(2) {
                    status.details.push(format!("VRAM: {}", mem.trim()));
                }
            }
            _ => {
                return DependencyStatus {
                    dep_type: DependencyType::Cuda,
                    status: CheckStatus::Missing,
                    version: None,
                    details: vec!["nvidia-smi not found — no NVIDIA GPU or drivers not installed".into()],
                    fix_hint: Some("Install NVIDIA drivers from https://www.nvidia.com/drivers".into()),
                };
            }
        }

        // Get CUDA version from nvidia-smi
        let cuda_ver = run_cmd_any("nvidia-smi", &[]).await;
        if let Some(full_output) = cuda_ver {
            // Look for "CUDA Version: 12.4"
            for line in full_output.lines() {
                if let Some(pos) = line.find("CUDA Version:") {
                    let ver = line[pos + 14..].trim().to_string();
                    status.version = Some(ver.clone());
                    status.details.push(format!("CUDA {}", ver));
                    break;
                }
            }
        }

        // Check if PyTorch can see CUDA (with 5s timeout)
        let torch_check = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::process::Command::new("python")
                .args(["-c", "import torch; print(f'torch {torch.__version__}, cuda={torch.cuda.is_available()}, devices={torch.cuda.device_count()}')"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output(),
        )
        .await;

        match torch_check {
            Ok(Ok(output)) if output.status.success() => {
                let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if out.contains("cuda=True") {
                    status.details.push(format!("PyTorch: {}", out));
                } else {
                    status.status = CheckStatus::NeedsSetup;
                    status.details.push(format!("PyTorch: {}", out));
                    status.fix_hint = Some(
                        "pip install torch --index-url https://download.pytorch.org/whl/cu124".into(),
                    );
                }
            }
            Ok(Ok(output)) => {
                let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if err.contains("No module named") {
                    status.details.push("PyTorch: not installed".into());
                } else {
                    status.details.push(format!("PyTorch check failed: {}", err.lines().next().unwrap_or("")));
                }
            }
            _ => {
                status.details.push("PyTorch: check timed out".into());
            }
        }

        status
    }
}
