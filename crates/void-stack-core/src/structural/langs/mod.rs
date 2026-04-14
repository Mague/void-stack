//! Per-language AST-node dispatch for the tree-sitter walker.
//!
//! Each supported language implements [`LanguageWalker`] with four
//! predicates that recognise class-like, function-like, call-like and
//! import-like nodes in its tree-sitter grammar. `parser::Walker`
//! dispatches to the right implementation via [`for_language`].
//!
//! This module also owns the language-identification entry points:
//! [`language_for`] (path → language id) and [`load_language`] (id →
//! tree-sitter `Language`), so `parser.rs` stays focused on walking.

use std::path::Path;

pub mod go;
pub mod javascript;
pub mod others;
pub mod python;
pub mod rust;

pub use go::GoWalker;
pub use javascript::JsWalker;
pub use others::{OtherLang, OthersWalker};
pub use python::PythonWalker;
pub use rust::RustWalker;

/// Predicates a language-specific walker must answer so the generic
/// AST-walk in `parser::Walker` can emit the right nodes / edges.
pub trait LanguageWalker: Send + Sync {
    fn is_class_node(&self, kind: &str) -> bool;
    fn is_function_node(&self, kind: &str) -> bool;
    fn is_call_node(&self, kind: &str) -> bool;
    fn is_import_node(&self, kind: &str) -> bool;
}

/// Map a path's extension to a tree-sitter language identifier.
/// Returns `None` for unsupported languages.
pub fn language_for(file_path: &Path) -> Option<&'static str> {
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" | "jsx" | "mjs" => Some("javascript"),
        "ts" => Some("typescript"),
        "tsx" => Some("tsx"),
        "go" => Some("go"),
        "dart" => Some("dart"),
        "java" => Some("java"),
        "php" | "phtml" => Some("php"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        _ => None,
    }
}

/// Resolve a language id (as returned by [`language_for`]) to the
/// tree-sitter `Language` handle required by the parser.
#[cfg(feature = "structural")]
pub fn load_language(lang: &str) -> Option<tree_sitter::Language> {
    match lang {
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "dart" => Some(tree_sitter_dart::LANGUAGE.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        "php" => Some(tree_sitter_php::LANGUAGE_PHP.into()),
        "c" => Some(tree_sitter_c::LANGUAGE.into()),
        "cpp" => Some(tree_sitter_cpp::LANGUAGE.into()),
        _ => None,
    }
}

/// Pick a walker for a language identifier (as returned by `language_for`).
/// Returns `None` for unsupported languages.
pub fn for_language(lang: &str) -> Option<Box<dyn LanguageWalker>> {
    match lang {
        "rust" => Some(Box::new(RustWalker)),
        "python" => Some(Box::new(PythonWalker)),
        "javascript" | "typescript" | "tsx" => Some(Box::new(JsWalker)),
        "go" => Some(Box::new(GoWalker)),
        "dart" => Some(Box::new(OthersWalker::new(OtherLang::Dart))),
        "java" => Some(Box::new(OthersWalker::new(OtherLang::Java))),
        "php" => Some(Box::new(OthersWalker::new(OtherLang::Php))),
        "c" => Some(Box::new(OthersWalker::new(OtherLang::C))),
        "cpp" => Some(Box::new(OthersWalker::new(OtherLang::Cpp))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_language_returns_walker_per_supported_lang() {
        for lang in [
            "rust",
            "python",
            "javascript",
            "typescript",
            "tsx",
            "go",
            "dart",
            "java",
            "php",
            "c",
            "cpp",
        ] {
            assert!(for_language(lang).is_some(), "{} must be supported", lang);
        }
    }

    #[test]
    fn for_language_none_on_unknown() {
        assert!(for_language("cobol").is_none());
        assert!(for_language("").is_none());
    }

    #[test]
    fn rust_walker_identifies_its_node_kinds() {
        let w = RustWalker;
        assert!(w.is_class_node("struct_item"));
        assert!(w.is_class_node("enum_item"));
        assert!(w.is_class_node("impl_item"));
        assert!(w.is_function_node("function_item"));
        assert!(w.is_call_node("call_expression"));
        assert!(w.is_call_node("macro_invocation"));
        assert!(w.is_import_node("use_declaration"));
        assert!(!w.is_class_node("call_expression"));
    }

    #[test]
    fn js_walker_covers_new_expression() {
        let w = JsWalker;
        assert!(w.is_call_node("call_expression"));
        assert!(w.is_call_node("new_expression"));
        assert!(w.is_function_node("arrow_function"));
    }

    #[test]
    fn others_walker_dispatches_by_lang() {
        let dart = OthersWalker::new(OtherLang::Dart);
        assert!(dart.is_class_node("class_declaration"));
        assert!(dart.is_function_node("function_signature"));

        let php = OthersWalker::new(OtherLang::Php);
        assert!(php.is_import_node("namespace_use_declaration"));
        assert!(php.is_call_node("member_call_expression"));

        let c = OthersWalker::new(OtherLang::C);
        assert!(c.is_import_node("preproc_include"));
        assert!(c.is_function_node("function_definition"));
    }
}
