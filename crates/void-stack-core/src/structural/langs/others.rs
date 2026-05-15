//! AST-node dispatch for languages with small per-language diffs:
//! Dart, Java, PHP, C, C++, Elixir. Grouped to avoid boilerplate — each
//! variant owns its own per-kind predicates.

use super::LanguageWalker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtherLang {
    Dart,
    Java,
    Php,
    C,
    Cpp,
    Elixir,
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
            // tree-sitter-elixir is a *minimal* grammar: defmodule /
            // defprotocol / defimpl all parse as `call` nodes whose first
            // child is the keyword identifier. There is no dedicated
            // "module" kind, so a kind-only predicate can't recognise
            // class-like blocks without inspecting child text. The File
            // node still gets emitted, which is enough to mark files as
            // parsed (the primary goal of Elixir support here).
            OtherLang::Elixir => false,
        }
    }

    fn is_function_node(&self, kind: &str) -> bool {
        match self.lang {
            OtherLang::Dart => kind == "function_signature",
            OtherLang::Java => matches!(kind, "method_declaration" | "constructor_declaration"),
            OtherLang::Php => matches!(kind, "function_definition" | "method_declaration"),
            OtherLang::C | OtherLang::Cpp => kind == "function_definition",
            // `anonymous_function` is the only standalone function-like
            // kind tree-sitter-elixir exposes (matches `fn x -> ... end`).
            // Top-level `def` / `defp` declarations parse as `call` and
            // are intentionally NOT matched here — extracting their name
            // would require inspecting the inner call (a parser-level
            // change beyond the `kind`-only predicate API).
            OtherLang::Elixir => kind == "anonymous_function",
        }
    }

    fn is_call_node(&self, kind: &str) -> bool {
        match self.lang {
            OtherLang::Dart => kind == "call_expression",
            OtherLang::Java => matches!(kind, "method_invocation" | "object_creation_expression"),
            OtherLang::Php => matches!(kind, "function_call_expression" | "member_call_expression"),
            OtherLang::C | OtherLang::Cpp => kind == "call_expression",
            // Every Elixir call (regular invocation AND def / defp / use /
            // alias / import macros) shows up as a `call` node. Treating
            // them all as call edges over-counts macro invocations but
            // is the only signal kind-only predicates can produce, and
            // populates the structural graph with usage info.
            OtherLang::Elixir => kind == "call",
        }
    }

    fn is_import_node(&self, kind: &str) -> bool {
        match self.lang {
            OtherLang::Dart => kind == "import_or_export",
            OtherLang::Java => kind == "import_declaration",
            OtherLang::Php => kind == "namespace_use_declaration",
            OtherLang::C | OtherLang::Cpp => kind == "preproc_include",
            // `alias`, `import`, `use`, `require` are all `call` nodes in
            // tree-sitter-elixir — indistinguishable from regular calls at
            // the kind level. Skip import edges; the keyword shows up as
            // the callee text on the existing call edges.
            OtherLang::Elixir => false,
        }
    }
}
