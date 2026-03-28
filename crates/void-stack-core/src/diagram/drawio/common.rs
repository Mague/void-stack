//! Shared constants, types, and helpers for Draw.io generation.

pub(crate) const FRONTEND_FILL: &str = "#d5e8d4";
pub(crate) const FRONTEND_STROKE: &str = "#82b366";
pub(crate) const BACKEND_FILL: &str = "#dae8fc";
pub(crate) const BACKEND_STROKE: &str = "#6c8ebf";
pub(crate) const DATABASE_FILL: &str = "#fff2cc";
pub(crate) const DATABASE_STROKE: &str = "#d6b656";
pub(crate) const WORKER_FILL: &str = "#e1d5e7";
pub(crate) const WORKER_STROKE: &str = "#9673a6";
pub(crate) const EXTERNAL_FILL: &str = "#f5f5f5";
pub(crate) const EXTERNAL_STROKE: &str = "#666666";
pub(crate) const CONTAINER_FILL: &str = "#dae8fc";
pub(crate) const CONTAINER_STROKE: &str = "#6c8ebf";

/// Cell ID generator for Draw.io XML elements.
pub(crate) struct IdGen(u32);

impl IdGen {
    pub(crate) fn new() -> Self {
        IdGen(2) // 0 and 1 are reserved
    }
    pub(crate) fn next(&mut self) -> u32 {
        let id = self.0;
        self.0 += 1;
        id
    }
}

/// XML-escape a string for use in attribute values.
pub(crate) fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
