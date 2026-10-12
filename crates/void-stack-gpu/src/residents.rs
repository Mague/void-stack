//! Los residentes: servidores de la casa que se quedan modelos en la GPU entre
//! trabajo y trabajo. Hoy, ComfyUI (el motor de render de OSAC).
//!
//! Medido el 2026-09-30 con ComfyUI 0.37.2 sin trabajo en cola: la GPU tenía
//! 13.626 MiB en uso con sólo el escritorio (1,9 GB) explicado; tras
//! `POST /free {"unload_models": true, "free_memory": true}` bajó a 4.087 MiB.
//! Es decir, ~9,5 GB de modelos en caché que el broker pintaba como intruso.
//! Su `/system_stats` no sirve para contarlos: con `cudaMallocAsync` y su
//! gestor de memoria, `torch_vram_total` decía 32 MB. Por eso la caché se
//! atribuye (ver `broker::Resident`) en vez de medirse.
//!
//! De ComfyUI se usan dos cosas de su API: `/queue` para saber si está en
//! pie y si trabaja, y `/free` para pedirle que suelte.

use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Default, Deserialize)]
struct QueueResponse {
    #[serde(default)]
    queue_running: Vec<serde_json::Value>,
    #[serde(default)]
    queue_pending: Vec<serde_json::Value>,
}

/// ¿Tiene trabajo? `None` si la respuesta no es la de una cola de ComfyUI.
pub fn parse_queue(body: &str) -> Option<bool> {
    serde_json::from_str::<QueueResponse>(body)
        .ok()
        .map(|q| !q.queue_running.is_empty() || !q.queue_pending.is_empty())
}

pub struct ComfyUi {
    base: String,
    http: reqwest::Client,
}

impl ComfyUi {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_default(),
        }
    }

    /// `None`: no está en pie. `Some(busy)`: contesta, y si tiene cola.
    pub async fn status(&self) -> Option<bool> {
        let body = self
            .http
            .get(format!("{}/queue", self.base))
            .send()
            .await
            .ok()?
            .text()
            .await
            .ok()?;
        parse_queue(&body)
    }

    /// Que suelte los modelos. Los vuelve a cargar solo en el próximo render.
    pub async fn free(&self) -> bool {
        self.http
            .post(format!("{}/free", self.base))
            .json(&serde_json::json!({ "unload_models": true, "free_memory": true }))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_cola_vacia_es_un_comfyui_ocioso() {
        assert_eq!(
            parse_queue(r#"{"queue_running": [], "queue_pending": []}"#),
            Some(false)
        );
    }

    #[test]
    fn con_algo_corriendo_o_esperando_trabaja() {
        assert_eq!(
            parse_queue(r#"{"queue_running": [[1, "abc"]], "queue_pending": []}"#),
            Some(true)
        );
        assert_eq!(
            parse_queue(r#"{"queue_running": [], "queue_pending": [[2, "x"]]}"#),
            Some(true)
        );
    }

    #[test]
    fn lo_que_no_es_una_cola_no_cuenta_como_comfyui() {
        assert_eq!(parse_queue("<html>no</html>"), None);
    }
}
