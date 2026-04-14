//! Data types shared by parser, graph store, and query helpers.
//!
//! Kept separate from `parser.rs` so the walker can focus on walking.
//! Every type here is `pub` / `pub use`-compatible with the pre-split API.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NodeKind {
    File,
    Class,
    Function,
    Test,
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeKind::File => "File",
            NodeKind::Class => "Class",
            NodeKind::Function => "Function",
            NodeKind::Test => "Test",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "File" => Some(NodeKind::File),
            "Class" => Some(NodeKind::Class),
            "Function" => Some(NodeKind::Function),
            "Test" => Some(NodeKind::Test),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EdgeKind {
    Calls,
    ImportsFrom,
    Inherits,
    Contains,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Calls => "CALLS",
            EdgeKind::ImportsFrom => "IMPORTS_FROM",
            EdgeKind::Inherits => "INHERITS",
            EdgeKind::Contains => "CONTAINS",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "CALLS" => Some(EdgeKind::Calls),
            "IMPORTS_FROM" => Some(EdgeKind::ImportsFrom),
            "INHERITS" => Some(EdgeKind::Inherits),
            "CONTAINS" => Some(EdgeKind::Contains),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuralNode {
    pub kind: NodeKind,
    pub name: String,
    /// `file::name` for top-level, `file::Class::method` for class members.
    pub qualified_name: String,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub language: String,
    pub parent_name: Option<String>,
    pub is_test: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuralEdge {
    pub kind: EdgeKind,
    pub source_qualified: String,
    pub target_qualified: String,
    pub file_path: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseResult {
    pub nodes: Vec<StructuralNode>,
    pub edges: Vec<StructuralEdge>,
}
