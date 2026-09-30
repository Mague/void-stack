//! `gpu.toml`: cómo se reparte la GPU.
//!
//! Vive junto a la configuración global de void-stack
//! (`%LOCALAPPDATA%\void-stack\gpu.toml`). Si no existe, todo tiene un valor
//! por defecto sensato y el broker arranca igual: un fichero que falta no
//! puede ser la razón de que nadie coordine la GPU.
//!
//! ── Las prioridades las pone el TOML, no el cliente ─────────────────────────
//! Un cliente dice qué prioridad quiere, pero si su dueño está en
//! `[priorities]`, manda el fichero. Si no, cualquier script podría declararse
//! `critical` y echar a MagueTrader de la GPU.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::broker::Limits;
use crate::model::{Priority, gb_to_mb};

pub const FILE_NAME: &str = "gpu.toml";

/// Loopback y puerto propio. NO el 7400: ese servidor MCP no tiene
/// autenticación y su código admite enlazarse por Tailscale. El broker da
/// órdenes de ceder y descargar; no puede quedar a un despiste de la red.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:7410";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GpuConfig {
    pub listen: String,
    /// Lo que usa el escritorio sin nada pesado. Medido el 2026-09-30: 1.821
    /// MiB con Chrome, Warp, WhatsApp y el shell abiertos. Se deja margen.
    pub baseline_gb: f64,
    pub lease_timeout_s: u64,
    pub queue_timeout_s: u64,
    /// Cada cuánto se mide la GPU y se pregunta a Ollama.
    pub sample_every_ms: u64,
    pub ollama_url: String,
    /// Memoria sin explicar a partir de la cual se habla de intruso. Por
    /// debajo es ruido: un navegador que abre una pestaña con vídeo.
    pub intruder_min_gb: f64,
    /// Ejecutables que, si están en la GPU sin lease, son sospechosos.
    ///
    /// Una lista y no "todo lo que esté en la GPU": en Windows (WDDM) los 24
    /// procesos del escritorio salen como `C+G` —Explorer, Chrome, WhatsApp,
    /// el propio Claude— y marcarlos a todos sería no marcar nada.
    pub intruder_watch: Vec<String>,
    /// Lo que usas tú: si uno de estos está en la GPU, modo juego.
    ///
    /// Una regla con barra (`\steamapps\common\`) busca ese trozo en la ruta
    /// del ejecutable; sin barra (`obs64.exe`) compara el nombre. Por ruta y no
    /// por nombre para los juegos: en esta máquina hay 18 bajo
    /// `steamapps\common` (medido el 2026-09-30) y una lista de nombres se
    /// quedaría vieja con el siguiente que instales. Los lanzadores (Steam,
    /// Riot Client) no cuentan: su interfaz también usa la GPU, pero viven
    /// fuera de esas carpetas.
    pub interactive: Vec<String>,
    pub priorities: BTreeMap<String, Priority>,
    /// Medir la GPU con NVML. Apagado reparte sólo por lo declarado: es lo
    /// que usan las pruebas, para no depender de lo que tenga abierto la
    /// máquina donde corren.
    pub measure: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        let priorities = [
            ("maguetrader", Priority::Critical),
            ("osac", Priority::Normal),
            ("terrain", Priority::Low),
            ("san-luis-terrain", Priority::Low),
            ("blender", Priority::Low),
            ("mcp_blender", Priority::Low),
            ("humboldt", Priority::Low),
            ("ollama", Priority::Opportunistic),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        Self {
            listen: DEFAULT_LISTEN.into(),
            baseline_gb: 1.9,
            lease_timeout_s: 60,
            queue_timeout_s: 60,
            sample_every_ms: 2_000,
            ollama_url: "http://127.0.0.1:11434".into(),
            intruder_min_gb: 1.0,
            intruder_watch: [
                "python.exe",
                "pythonw.exe",
                "python",
                "ollama.exe",
                "ollama_llama_server.exe",
                "blender.exe",
                "ffmpeg.exe",
                "comfyui",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            interactive: [
                "TikTok LIVE Studio.exe",
                "obs64.exe",
                "Streamlabs OBS.exe",
                "League of Legends.exe",
                "VALORANT-Win64-Shipping.exe",
                r"\steamapps\common\",
                r"\Riot Games\League of Legends\Game\",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            priorities,
            measure: true,
        }
    }
}

impl GpuConfig {
    pub fn limits(&self) -> Limits {
        Limits {
            baseline_mb: gb_to_mb(self.baseline_gb),
            lease_timeout_ms: self.lease_timeout_s * 1_000,
            queue_timeout_ms: self.queue_timeout_s * 1_000,
            ..Limits::default()
        }
    }

    /// La prioridad que manda: la del fichero si el dueño está, si no la pedida.
    pub fn priority_for(&self, owner: &str, asked: Priority) -> Priority {
        self.priorities
            .get(&owner.to_ascii_lowercase())
            .copied()
            .unwrap_or(asked)
    }

    pub fn intruder_min_mb(&self) -> u32 {
        gb_to_mb(self.intruder_min_gb)
    }
}

pub fn default_path() -> Option<PathBuf> {
    void_stack_core::global_config::global_config_dir()
        .ok()
        .map(|d| d.join(FILE_NAME))
}

/// Lee el fichero; si falta o está mal, lo dice y sigue con los valores por
/// defecto. Un `gpu.toml` roto no puede dejar la GPU sin árbitro.
pub fn load(path: Option<&Path>) -> (GpuConfig, Option<String>) {
    let Some(path) = path else {
        return (GpuConfig::default(), None);
    };
    match std::fs::read_to_string(path) {
        Err(_) => (GpuConfig::default(), None),
        Ok(text) => match toml::from_str::<GpuConfig>(&text) {
            Ok(cfg) => (cfg, None),
            Err(e) => (
                GpuConfig::default(),
                Some(format!(
                    "{} no se pudo leer ({e}); uso los valores por defecto",
                    path.display()
                )),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_fichero_arranca_con_lo_de_por_defecto() {
        let (cfg, warning) = load(Some(Path::new("no/existe/gpu.toml")));
        assert_eq!(cfg, GpuConfig::default());
        assert!(warning.is_none());
    }

    #[test]
    fn un_fichero_roto_avisa_pero_no_deja_la_gpu_sin_arbitro() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(FILE_NAME);
        std::fs::write(&p, "baseline_gb = \"mucho\"").unwrap();
        let (cfg, warning) = load(Some(&p));
        assert_eq!(cfg, GpuConfig::default());
        assert!(warning.unwrap().contains("no se pudo leer"));
    }

    #[test]
    fn lo_que_no_se_escribe_toma_su_valor_por_defecto() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(FILE_NAME);
        std::fs::write(&p, "baseline_gb = 2.5\n[priorities]\nosac = \"critical\"\n").unwrap();
        let (cfg, _) = load(Some(&p));
        assert_eq!(cfg.baseline_gb, 2.5);
        assert_eq!(cfg.listen, DEFAULT_LISTEN);
        assert_eq!(cfg.priorities.get("osac"), Some(&Priority::Critical));
    }

    #[test]
    fn un_cliente_no_puede_declararse_critico_si_el_fichero_dice_otra_cosa() {
        let cfg = GpuConfig::default();
        assert_eq!(
            cfg.priority_for("osac", Priority::Critical),
            Priority::Normal
        );
        assert_eq!(
            cfg.priority_for("OSAC", Priority::Critical),
            Priority::Normal
        );
        // Quien no está en el fichero pide lo que quiera.
        assert_eq!(cfg.priority_for("nuevo", Priority::Low), Priority::Low);
    }

    #[test]
    fn el_broker_escucha_solo_en_loopback() {
        assert!(GpuConfig::default().listen.starts_with("127.0.0.1:"));
    }
}
