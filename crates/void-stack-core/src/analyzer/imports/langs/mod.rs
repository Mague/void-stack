//! One import parser per language. `build_graph` in the parent module
//! picks the parsers that apply to a project and links their imports.

pub mod cpp;
pub mod csharp;
pub mod dart;
pub mod gdscript;
pub mod golang;
pub mod javascript;
pub mod python;
pub mod rust_lang;
pub mod swift;
pub mod verse;
