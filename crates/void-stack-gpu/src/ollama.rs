//! Ollama, la llama dormilona: se la descarga cuando otro necesita la memoria.
//!
//! Es lo que Humboldt aprendió a golpes (`VRAM_OPTIMIZATION.md`): el pipeline
//! tardaba 4 horas porque Ollama retenía 5+ GB de modelos mientras EasyOCR
//! intentaba correr. La solución fue descargar con `keep_alive: 0` entre
//! fases. Aquí se hace lo mismo, pero decidido desde fuera y para todos.
//!
//! `/api/ps` dice qué modelos hay cargados y cuánta VRAM ocupa cada uno
//! (`size_vram`), y eso es exactamente lo que el broker necesita para contar.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LoadedModel {
    pub name: String,
    #[serde(default)]
    pub size_vram: u64,
}

#[derive(Debug, Deserialize)]
struct PsResponse {
    #[serde(default)]
    models: Vec<LoadedModel>,
}

/// Lo que ocupa Ollama ahora mismo: (MB en VRAM, nombres).
pub fn summarize(models: &[LoadedModel]) -> (u32, Vec<String>) {
    let mb = models
        .iter()
        .map(|m| m.size_vram / (1024 * 1024))
        .sum::<u64>() as u32;
    (mb, models.iter().map(|m| m.name.clone()).collect())
}

pub fn parse_ps(body: &str) -> Vec<LoadedModel> {
    serde_json::from_str::<PsResponse>(body)
        .map(|r| r.models)
        .unwrap_or_default()
}

pub struct Ollama {
    base: String,
    http: reqwest::Client,
}

impl Ollama {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_owned(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Los modelos cargados. Si Ollama no está corriendo, ninguno: apagado
    /// ocupa lo mismo que descargado.
    pub async fn loaded(&self) -> Vec<LoadedModel> {
        match self.http.get(format!("{}/api/ps", self.base)).send().await {
            Ok(r) => parse_ps(&r.text().await.unwrap_or_default()),
            Err(_) => Vec::new(),
        }
    }

    /// Descarga cada modelo con `keep_alive: 0`, igual que Humboldt.
    pub async fn unload_all(&self) -> Vec<String> {
        let mut done = Vec::new();
        for model in self.loaded().await {
            let body = serde_json::json!({ "model": model.name, "keep_alive": 0 });
            let ok = self
                .http
                .post(format!("{}/api/generate", self.base))
                .json(&body)
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
            if ok {
                done.push(model.name);
            }
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_lo_que_devuelve_api_ps() {
        let body = r#"{"models":[
            {"name":"qwen2.5vl:7b","model":"qwen2.5vl:7b","size":6000000000,"size_vram":5368709120},
            {"name":"nomic-embed-text","size_vram":314572800}
        ]}"#;
        let models = parse_ps(body);
        assert_eq!(models.len(), 2);
        assert_eq!(
            summarize(&models),
            (
                5_120 + 300,
                vec!["qwen2.5vl:7b".into(), "nomic-embed-text".into()]
            )
        );
    }

    #[test]
    fn ollama_apagado_o_raro_es_cero_modelos() {
        assert!(parse_ps("").is_empty());
        assert!(parse_ps("<html>no</html>").is_empty());
        assert_eq!(summarize(&[]), (0, vec![]));
    }

    #[tokio::test]
    async fn sin_ollama_corriendo_no_hay_nada_cargado_ni_se_revienta() {
        // Un puerto donde no escucha nadie.
        let o = Ollama::new("http://127.0.0.1:9");
        assert!(o.loaded().await.is_empty());
        assert!(o.unload_all().await.is_empty());
    }
}
