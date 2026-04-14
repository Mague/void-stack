//! Rust language AST-node dispatch.

use super::LanguageWalker;

pub struct RustWalker;

impl LanguageWalker for RustWalker {
    fn is_class_node(&self, kind: &str) -> bool {
        matches!(kind, "struct_item" | "enum_item" | "impl_item")
    }

    fn is_function_node(&self, kind: &str) -> bool {
        kind == "function_item"
    }

    fn is_call_node(&self, kind: &str) -> bool {
        matches!(kind, "call_expression" | "macro_invocation")
    }

    fn is_import_node(&self, kind: &str) -> bool {
        kind == "use_declaration"
    }
}
