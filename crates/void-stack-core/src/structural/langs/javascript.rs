//! JavaScript / TypeScript / TSX AST-node dispatch. All three share the
//! same class / function / call / import node kinds in their respective
//! tree-sitter grammars.

use super::LanguageWalker;

pub struct JsWalker;

impl LanguageWalker for JsWalker {
    fn is_class_node(&self, kind: &str) -> bool {
        matches!(kind, "class_declaration" | "class")
    }

    fn is_function_node(&self, kind: &str) -> bool {
        matches!(
            kind,
            "function_declaration" | "method_definition" | "arrow_function"
        )
    }

    fn is_call_node(&self, kind: &str) -> bool {
        matches!(kind, "call_expression" | "new_expression")
    }

    fn is_import_node(&self, kind: &str) -> bool {
        kind == "import_statement"
    }
}
