//! Project inventory without an index: the map of a project (packages,
//! entry files, directory tree, key docs) and its entry points (routes,
//! RPCs, MCP tools, CLI commands, UI routes, jobs).

mod entrypoints;
mod map;
mod walk;

pub use entrypoints::{EntryKind, EntryPoint, entrypoints, render as render_entrypoints};
pub use map::{Package, packages, project_map};
