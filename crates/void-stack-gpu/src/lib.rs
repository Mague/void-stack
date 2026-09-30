//! void-stack-gpu: un broker para una sola GPU compartida.
//!
//! Generaliza lo que Humboldt ya resolvió dentro de sí mismo
//! (`humboldt/infrastructure/gpu_utils.py` + `VRAM_OPTIMIZATION.md`): allí un
//! `asyncio.Lock` impedía que OCR, LLM y visión se pisaran la VRAM y el pipeline
//! pasó de 4 horas a menos de 10 minutos por PDF. Pero ese lock sólo vale
//! DENTRO de un proceso. Aquí compiten MagueTrader, OSAC, el terreno, Blender,
//! Ollama y Humboldt: procesos distintos, lenguajes distintos. Hace falta alguien
//! fuera de todos ellos que lleve la cuenta.

pub mod agents;
pub mod broker;
pub mod config;
pub mod history;
pub mod model;
pub mod ollama;
pub mod probe;
pub mod server;

pub use server::{run, spawn};
