//! Go language AST-node dispatch.

use super::LanguageWalker;

pub struct GoWalker;

impl LanguageWalker for GoWalker {
    fn is_class_node(&self, kind: &str) -> bool {
        kind == "type_declaration"
    }

    fn is_function_node(&self, kind: &str) -> bool {
        matches!(kind, "function_declaration" | "method_declaration")
    }

    fn is_call_node(&self, kind: &str) -> bool {
        kind == "call_expression"
    }

    fn is_import_node(&self, kind: &str) -> bool {
        kind == "import_declaration"
    }
}
