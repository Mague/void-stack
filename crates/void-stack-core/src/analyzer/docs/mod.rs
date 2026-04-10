//! Markdown documentation generation from analysis results.
//!
//! Split into submodules:
//! - `markdown` — section-by-section markdown generation
//! - `coverage` — language-specific coverage hints
//! - `sanitize` — Mermaid diagram ID sanitization

mod coverage;
mod markdown;
mod sanitize;

pub use markdown::{generate_docs, generate_docs_compact, generate_docs_full};
