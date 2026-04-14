//! Python language AST-node dispatch.

use super::LanguageWalker;

pub struct PythonWalker;

impl LanguageWalker for PythonWalker {
    fn is_class_node(&self, kind: &str) -> bool {
        kind == "class_definition"
    }

    fn is_function_node(&self, kind: &str) -> bool {
        kind == "function_definition"
    }

    fn is_call_node(&self, kind: &str) -> bool {
        kind == "call"
    }

    fn is_import_node(&self, kind: &str) -> bool {
        matches!(kind, "import_statement" | "import_from_statement")
    }
}
