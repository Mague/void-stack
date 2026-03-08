use std::path::Path;

use async_trait::async_trait;

use super::{run_cmd, CheckStatus, DependencyDetector, DependencyStatus, DependencyType};

pub struct OllamaDetector;

#[async_trait]
impl DependencyDetector for OllamaDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Ollama
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        // Check if any source file references ollama
        let files = ["requirements.txt", "pyproject.toml", ".env", ".env.example"];
        for file in &files {
            if let Ok(content) = std::fs::read_to_string(project_path.join(file)) {
                let lower = content.to_lowercase();
                if lower.contains("ollama") {
                    return true;
                }
            }
        }
        // Also check common Python entry files
        let py_files = ["main.py", "app.py", "server.py"];
        for file in &py_files {
            if let Ok(content) = std::fs::read_to_string(project_path.join(file)) {
                if content.contains("ollama") {
                    return true;
                }
            }
        }
        false
    }

    async fn check(&self, _project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Ollama);

        // Check if ollama binary exists
        let ollama_ver = run_cmd("ollama", &["--version"]).await;
        match ollama_ver {
            Some(ver) => {
                // "ollama version is 0.3.12" → "0.3.12"
                let ver_clean = ver
                    .strip_prefix("ollama version is ")
                    .unwrap_or(&ver)
                    .to_string();
                status.version = Some(ver_clean.clone());
                status.details.push(format!("Ollama {}", ver_clean));
            }
            None => {
                return DependencyStatus {
                    dep_type: DependencyType::Ollama,
                    status: CheckStatus::Missing,
                    version: None,
                    details: vec!["Ollama not found in PATH".into()],
                    fix_hint: Some("winget install Ollama.Ollama".into()),
                };
            }
        }

        // Check if Ollama service is running via API
        let api_check = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            check_ollama_api(),
        )
        .await;

        match api_check {
            Ok(Some(models)) => {
                if models.is_empty() {
                    status.details.push("Service running, no models downloaded".into());
                } else {
                    status
                        .details
                        .push(format!("Models: {}", models.join(", ")));
                }
            }
            Ok(None) => {
                status.status = CheckStatus::NotRunning;
                status.details.push("Service not running".into());
                status.fix_hint = Some("ollama serve".into());
            }
            Err(_) => {
                status.status = CheckStatus::NotRunning;
                status.details.push("API check timed out".into());
                status.fix_hint = Some("ollama serve".into());
            }
        }

        status
    }
}

/// Check Ollama API and return list of model names, or None if not running.
async fn check_ollama_api() -> Option<Vec<String>> {
    // Use a simple TCP connection + HTTP request to avoid requiring reqwest
    let output = tokio::process::Command::new("curl")
        .args(["-s", "--max-time", "2", "http://localhost:11434/api/tags"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let body = String::from_utf8_lossy(&output.stdout);
    if body.is_empty() {
        return None;
    }

    // Parse JSON to extract model names
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let models = json
        .get("models")?
        .as_array()?
        .iter()
        .filter_map(|m| m.get("name")?.as_str().map(|s| s.to_string()))
        .collect();

    Some(models)
}
