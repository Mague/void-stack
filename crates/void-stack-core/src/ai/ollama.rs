//! Ollama provider: HTTP client for the Ollama local LLM API.
//!
//! Sends requests to `http://localhost:11434/api/generate` (or configured URL).

use super::{SuggestionResult, parse_suggestions};
use crate::error::{Result, VoidStackError};

/// Generate suggestions by calling the Ollama API.
///
/// POST to `/api/generate` with the model and prompt.
/// Parses the streaming NDJSON response.
pub async fn generate(base_url: &str, model: &str, prompt: &str) -> Result<SuggestionResult> {
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "options": {
            "temperature": 0.3,
            "num_predict": 2048,
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| VoidStackError::RunnerError(format!("HTTP client error: {}", e)))?;

    let response = client.post(&url).json(&body).send().await.map_err(|e| {
        if e.is_connect() {
            VoidStackError::RunnerError(format!(
                "No se pudo conectar a Ollama en {}. ¿Está corriendo? (ollama serve)",
                base_url
            ))
        } else if e.is_timeout() {
            VoidStackError::RunnerError(
                "Timeout esperando respuesta de Ollama (120s). Intenta con un modelo más pequeño."
                    .to_string(),
            )
        } else {
            VoidStackError::RunnerError(format!("Error de red con Ollama: {}", e))
        }
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        if status.as_u16() == 404 && body_text.contains("not found") {
            // Model not downloaded — list available models for hint
            let available = list_models(base_url).await.unwrap_or_default();
            let hint = if available.is_empty() {
                format!("Ejecuta: ollama pull {}", model)
            } else {
                format!(
                    "Modelos disponibles: {}. O descarga con: ollama pull {}",
                    available.join(", "),
                    model
                )
            };
            return Err(VoidStackError::RunnerError(format!(
                "Modelo '{}' no encontrado en Ollama. {}",
                model, hint
            )));
        }
        return Err(VoidStackError::RunnerError(format!(
            "Ollama respondió con HTTP {}: {}",
            status, body_text
        )));
    }

    let resp_body: serde_json::Value = response.json().await.map_err(|e| {
        VoidStackError::RunnerError(format!("Error parseando respuesta de Ollama: {}", e))
    })?;

    let raw_response = resp_body["response"].as_str().unwrap_or("").to_string();

    if raw_response.is_empty() {
        return Err(VoidStackError::RunnerError(
            "Ollama devolvió una respuesta vacía. Verifica que el modelo esté descargado."
                .to_string(),
        ));
    }

    let suggestions = parse_suggestions(&raw_response);

    Ok(SuggestionResult {
        suggestions,
        model_used: model.to_string(),
        raw_response,
    })
}

/// Check if Ollama is reachable at the given base URL.
pub async fn is_available(base_url: &str) -> bool {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build();

    match client {
        Ok(c) => c
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// List available models from Ollama.
pub async fn list_models(base_url: &str) -> Result<Vec<String>> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| VoidStackError::RunnerError(format!("HTTP client error: {}", e)))?;

    let response =
        client.get(&url).send().await.map_err(|e| {
            VoidStackError::RunnerError(format!("No se pudo conectar a Ollama: {}", e))
        })?;

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| VoidStackError::RunnerError(format!("Error parseando modelos: {}", e)))?;

    let models = body["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}
