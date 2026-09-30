//! Tipos que cruzan la frontera del broker: lo que pide un cliente, lo que
//! contesta, y la foto que ve La Oficina.
//!
//! La VRAM viaja en **megabytes enteros** por dentro. Los clientes la piden en
//! GB (`vram_gb: 6.5`) porque asi se piensa, pero sumar flotantes para decidir
//! si "cabe" es la forma clasica de que 16.0 no quepa en 16.0.

use serde::{Deserialize, Serialize};

/// Quién manda cuando no cabe todo.
///
/// El orden de las variantes ES el orden de prioridad: `Critical` primero. Se
/// deriva `Ord` a propósito para que ordenar la cola sea ordenar por esto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Nunca se interrumpe; cede sólo en sus propios puntos de control.
    Critical,
    /// Cede entre fragmentos si llega algo crítico.
    Normal,
    Low,
    /// Se descarga cuando alguien de más prioridad necesita la memoria.
    Opportunistic,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Priority::Critical => "crítica",
            Priority::Normal => "normal",
            Priority::Low => "baja",
            Priority::Opportunistic => "oportunista",
        }
    }
}

/// Lo que pide un cliente al entrar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseRequest {
    /// El proyecto, tal cual lo nombra la oficina: `osac`, `maguetrader`…
    pub owner: String,
    /// Qué está haciendo, en una línea: `render expedientes/02`.
    pub task: String,
    pub vram_gb: f64,
    pub priority: Priority,
    /// Si puede soltar la GPU en un `yield_point` cuando se lo pidan.
    #[serde(default)]
    pub preemptible: bool,
    /// El PID que va a usar la GPU, cuando el cliente lo sabe.
    #[serde(default)]
    pub pid: Option<u32>,
    /// Dónde vive DE VERDAD la VRAM, si no es en el propio proceso.
    ///
    /// Medido al integrar: OSAC renderiza dentro de ComfyUI y el LLM de
    /// Humboldt dentro de Ollama. Sin esto, el broker descargaría a Ollama
    /// "por oportunista" en mitad del trabajo de quien lo está usando, y
    /// además contaría esa memoria dos veces.
    #[serde(default)]
    pub backend: Option<String>,
}

impl LeaseRequest {
    pub fn vram_mb(&self) -> u32 {
        gb_to_mb(self.vram_gb)
    }
}

pub fn gb_to_mb(gb: f64) -> u32 {
    if !gb.is_finite() || gb <= 0.0 {
        return 0;
    }
    (gb * 1024.0).round() as u32
}

pub fn mb_to_gb(mb: u32) -> f64 {
    (mb as f64 / 1024.0 * 10.0).round() / 10.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseState {
    Queued,
    Granted,
}

/// Lo que el broker le dice a un cliente cada vez que pregunta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Directive {
    /// Sigue.
    Continue,
    /// Suelta la GPU en tu próximo punto seguro y vuelve a la fila.
    Yield,
    /// Para: alguien lo canceló desde La Oficina.
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub id: String,
    pub owner: String,
    pub task: String,
    pub vram_mb: u32,
    pub priority: Priority,
    pub preemptible: bool,
    pub pid: Option<u32>,
    pub backend: Option<String>,
    pub state: LeaseState,
    pub directive: Directive,
    /// Por qué se le pidió ceder, para poder decirlo en la ficha.
    pub reason: Option<String>,
    pub requested_at_ms: u64,
    pub granted_at_ms: Option<u64>,
    pub last_seen_ms: u64,
    /// 0..1, cuando el cliente lo informa.
    pub progress: Option<f32>,
    pub message: Option<String>,
    /// El usuario lo puso en pausa desde La Oficina.
    pub held: bool,
}

/// Cómo terminó un lease. Va al historial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// El cliente lo soltó al terminar.
    Released,
    /// El cliente lo soltó porque le pidieron ceder; vuelve a la fila.
    Yielded,
    /// Dejó de dar señales: el proceso murió o se colgó.
    Expired,
    Cancelled,
}

/// Una fila del historial: quién ocupó la GPU, cuánto, y cuánto esperó.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryEntry {
    pub lease_id: String,
    pub owner: String,
    pub task: String,
    pub vram_mb: u32,
    pub priority: Priority,
    pub requested_at_ms: u64,
    /// Nulo si nunca llegó a entrar (caducó o se canceló en la fila).
    pub granted_at_ms: Option<u64>,
    pub ended_at_ms: u64,
    pub outcome: Outcome,
    /// Cuántos turnos seguidos resume esta fila. Ver `history::BURST_GAP_MS`.
    #[serde(default = "one")]
    pub turns: u32,
}

fn one() -> u32 {
    1
}

impl HistoryEntry {
    /// Cuánto esperó en la fila, en ms.
    pub fn waited_ms(&self) -> u64 {
        self.granted_at_ms
            .unwrap_or(self.ended_at_ms)
            .saturating_sub(self.requested_at_ms)
    }
}

/// Algo que el núcleo decide pero no ejecuta.
///
/// El núcleo es puro: no hace red ni toca procesos. Devuelve efectos y la capa
/// asíncrona los cumple. Así los escenarios del spec se prueban sin GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Descargar los modelos que Ollama tiene en VRAM (`keep_alive: 0`).
    UnloadOllama { reason: String },
    /// Pedir a un residente (ComfyUI) que suelte los modelos de su caché.
    FreeResident { name: String, reason: String },
    /// Algo cambió: avisar a quien esté mirando.
    Changed,
}
