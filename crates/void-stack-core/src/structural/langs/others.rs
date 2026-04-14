//! AST-node dispatch for languages with small per-language diffs:
//! Dart, Java, PHP, C, C++. Grouped to avoid boilerplate — each variant
//! owns its own per-kind predicates.

use super::LanguageWalker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtherLang {
    Dart,
    Java,
    Php,
    C,
    Cpp,
}

pub struct OthersWalker {
    lang: OtherLang,
}

impl OthersWalker {
    pub fn new(lang: OtherLang) -> Self {
        Self { lang }
    }
}

impl LanguageWalker for OthersWalker {
    fn is_class_node(&self, kind: &str) -> bool {
        match self.lang {
            OtherLang::Dart => matches!(
                kind,
                "class_declaration" | "class_definition" | "mixin_declaration" | "enum_declaration"
            ),
            OtherLang::Java => matches!(
                kind,
                "class_declaration" | "interface_declaration" | "enum_declaration"
            ),
            OtherLang::Php => matches!(kind, "class_declaration" | "interface_declaration"),
            OtherLang::C => matches!(kind, "struct_specifier" | "type_definition"),
            OtherLang::Cpp => matches!(kind, "class_specifier" | "struct_specifier"),
        }
    }

    fn is_function_node(&self, kind: &str) -> bool {
        match self.lang {
            OtherLang::Dart => kind == "function_signature",
            OtherLang::Java => matches!(kind, "method_declaration" | "constructor_declaration"),
            OtherLang::Php => matches!(kind, "function_definition" | "method_declaration"),
            OtherLang::C | OtherLang::Cpp => kind == "function_definition",
        }
    }

    fn is_call_node(&self, kind: &str) -> bool {
        match self.lang {
            OtherLang::Dart => kind == "call_expression",
            OtherLang::Java => matches!(kind, "method_invocation" | "object_creation_expression"),
            OtherLang::Php => matches!(kind, "function_call_expression" | "member_call_expression"),
            OtherLang::C | OtherLang::Cpp => kind == "call_expression",
        }
    }

    fn is_import_node(&self, kind: &str) -> bool {
        match self.lang {
            OtherLang::Dart => kind == "import_or_export",
            OtherLang::Java => kind == "import_declaration",
            OtherLang::Php => kind == "namespace_use_declaration",
            OtherLang::C | OtherLang::Cpp => kind == "preproc_include",
        }
    }
}
