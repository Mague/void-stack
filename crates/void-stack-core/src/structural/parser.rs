//! Tree-sitter powered structural parser.
//!
//! Walks the AST of supported languages and emits nodes (files, classes,
//! functions, tests) plus edges (calls, imports, inherits, contains).
//! Call targets start out as bare names and are resolved to qualified names
//! using the same-file symbol table before returning.

// AST node type mappings and Tree-sitter extraction patterns adapted from
// code-review-graph by Tirth Patel (tirth8205)
// https://github.com/tirth8205/code-review-graph — MIT License
// Reimplemented natively in Rust using tree-sitter crate bindings.

use std::path::Path;

#[cfg(feature = "structural")]
use tree_sitter::{Node, Parser, Tree};

#[cfg(feature = "structural")]
use super::langs::{self, LanguageWalker};

pub use super::langs::language_for;
pub use super::model::{EdgeKind, NodeKind, ParseResult, StructuralEdge, StructuralNode};

/// Very small test-detection heuristic mirroring code-review-graph's patterns.
/// Prefers the function name, then falls back to the file path.
pub fn is_test_function(name: &str, file_path: &str) -> bool {
    let lower = name;
    if lower.starts_with("test_") || lower.starts_with("Test") || lower.ends_with("_test") {
        return true;
    }
    let fp = file_path;
    if fp.contains("/tests/")
        || fp.contains("\\tests\\")
        || fp.starts_with("tests/")
        || fp.starts_with("tests\\")
    {
        return true;
    }
    if fp.ends_with("_test.go")
        || fp.ends_with(".test.ts")
        || fp.ends_with(".test.tsx")
        || fp.ends_with(".test.js")
        || fp.ends_with(".test.jsx")
        || fp.ends_with(".spec.ts")
        || fp.ends_with(".spec.js")
        || fp.ends_with(".spec.tsx")
        || fp.ends_with(".spec.jsx")
    {
        return true;
    }
    if fp.starts_with("test_") && fp.ends_with(".py") {
        return true;
    }
    if fp.ends_with("_test.py") {
        return true;
    }
    false
}

/// Build a qualified name following the project convention
/// `file::parent::name` (or `file::name` when there is no parent).
pub fn qualify(file_path: &str, name: &str, parent: Option<&str>) -> String {
    match parent {
        Some(p) if !p.is_empty() => format!("{}::{}::{}", file_path, p, name),
        _ => format!("{}::{}", file_path, name),
    }
}

#[cfg(feature = "structural")]
struct Walker<'a> {
    source: &'a [u8],
    file_path: String,
    language: String,
    lang_walker: Box<dyn LanguageWalker>,
    nodes: Vec<StructuralNode>,
    edges: Vec<StructuralEdge>,
    /// Body nodes already walked under their function's scope. Dart puts
    /// `function_body` as a SIBLING of `function_signature` (not a child),
    /// so the function handler walks it eagerly and records it here to keep
    /// the parent loop from re-walking it without the function context.
    consumed_bodies: std::collections::HashSet<usize>,
}

#[cfg(feature = "structural")]
impl<'a> Walker<'a> {
    fn node_text(&self, n: Node<'_>) -> &str {
        std::str::from_utf8(&self.source[n.byte_range()]).unwrap_or("")
    }

    fn child_name(&self, n: Node<'_>) -> Option<String> {
        // Prefer the explicit "name" field; many grammars expose it directly.
        if let Some(name_node) = n.child_by_field_name("name") {
            return Some(self.node_text(name_node).to_string());
        }
        // C/C++ put the identifier inside `declarator` → `function_declarator`
        // → `identifier`. Recurse into `declarator` fields to unwrap it.
        if let Some(declarator) = n.child_by_field_name("declarator") {
            if let Some(name) = self.child_name(declarator) {
                return Some(name);
            }
            let text = self.node_text(declarator).trim();
            if !text.is_empty() {
                return Some(text.split('(').next().unwrap_or(text).trim().to_string());
            }
        }
        // Fall back to the first identifier-shaped child.
        let mut cursor = n.walk();
        for child in n.children(&mut cursor) {
            let kind = child.kind();
            if kind == "identifier"
                || kind == "type_identifier"
                || kind == "field_identifier"
                || kind == "property_identifier"
            {
                return Some(self.node_text(child).to_string());
            }
        }
        None
    }

    fn is_class_node(&self, kind: &str) -> bool {
        self.lang_walker.is_class_node(kind)
    }

    fn is_function_node(&self, kind: &str) -> bool {
        self.lang_walker.is_function_node(kind)
    }

    fn is_call_node(&self, kind: &str) -> bool {
        self.lang_walker.is_call_node(kind)
    }

    fn is_import_node(&self, kind: &str) -> bool {
        self.lang_walker.is_import_node(kind)
    }

    fn extract_call_target(&self, call: Node<'_>) -> Option<String> {
        // Dart has no invocation node: a call is a `selector` carrying an
        // `argument_part`, and the callee is whatever sits immediately
        // before it — a bare identifier (`doLocal()`), a `.method` selector
        // (`_auth.loginWithGoogle()`), or a type identifier (`AuthService()`).
        if call.kind() == "selector" {
            if !self.has_child_of_kind(call, "argument_part") {
                return None; // plain `.field` access, not a call
            }
            let prev = call.prev_sibling()?;
            return self.last_identifier_text(prev);
        }

        // `function` field holds the callee for call_expression; for macro/new
        // we fall back to the first non-trivial child identifier.
        if let Some(func) = call.child_by_field_name("function") {
            return Some(self.callee_bare_name(func));
        }
        let mut cursor = call.walk();
        for child in call.children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "type_identifier" {
                return Some(self.node_text(child).to_string());
            }
        }
        None
    }

    fn has_child_of_kind(&self, n: Node<'_>, kind: &str) -> bool {
        let mut cursor = n.walk();
        n.children(&mut cursor).any(|c| c.kind() == kind)
    }

    /// Deepest-last identifier in a node — for Dart callee extraction the
    /// previous sibling is either an identifier itself or a selector whose
    /// last identifier is the method name.
    fn last_identifier_text(&self, n: Node<'_>) -> Option<String> {
        if matches!(n.kind(), "identifier" | "type_identifier") {
            return Some(self.node_text(n).to_string());
        }
        let mut cursor = n.walk();
        let children: Vec<Node<'_>> = n.children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            if let Some(t) = self.last_identifier_text(child) {
                return Some(t);
            }
        }
        None
    }

    fn callee_bare_name(&self, n: Node<'_>) -> String {
        let text = self.node_text(n).trim();
        // Split a path/member chain into segments and keep the tail. When
        // the segment BEFORE the tail looks like a type (`Foo::new`,
        // `AuthService.fromJson`), keep it as a `Type::tail` receiver hint —
        // it lets the resolver tell `Foo::new` apart from `Bar::new`
        // instead of attributing every `.new(` in the repo to both.
        let segments: Vec<&str> = text
            .split(['.', ':'])
            .filter(|s| !s.is_empty())
            .map(str::trim)
            .collect();
        match segments.as_slice() {
            [] => text.to_string(),
            [only] => only.to_string(),
            [.., recv, tail] => {
                let is_type_like = recv.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && recv.chars().all(|c| c.is_alphanumeric() || c == '_');
                if is_type_like {
                    format!("{}::{}", recv, tail)
                } else {
                    tail.to_string()
                }
            }
        }
    }

    fn import_module(&self, n: Node<'_>) -> Option<String> {
        // Most grammars expose a `name`, `module_name`, or `path` field.
        for field in &["name", "module_name", "path"] {
            if let Some(node) = n.child_by_field_name(field) {
                return Some(
                    self.node_text(node)
                        .trim_matches(|c: char| {
                            c == '"' || c == '\'' || c == '<' || c == '>' || c.is_whitespace()
                        })
                        .to_string(),
                );
            }
        }
        let mut cursor = n.walk();
        for child in n.children(&mut cursor) {
            match child.kind() {
                "interpreted_string_literal"
                | "string"
                | "string_literal"
                | "import_path"
                | "dotted_name"
                | "scoped_identifier"
                | "identifier"
                | "system_lib_string"
                | "uri" => {
                    return Some(
                        self.node_text(child)
                            .trim_matches(|c: char| {
                                c == '"' || c == '\'' || c == '<' || c == '>' || c.is_whitespace()
                            })
                            .to_string(),
                    );
                }
                _ => {}
            }
        }
        None
    }

    fn walk(
        &mut self,
        node: Node<'_>,
        enclosing_class: Option<&str>,
        enclosing_func_qn: Option<&str>,
        file_qn: &str,
    ) {
        if self.consumed_bodies.contains(&node.id()) {
            return;
        }
        let kind = node.kind();

        // Imports emit a file-level edge and don't recurse into children.
        if self.is_import_node(kind)
            && let Some(module) = self.import_module(node)
        {
            self.edges.push(StructuralEdge {
                kind: EdgeKind::ImportsFrom,
                source_qualified: file_qn.to_string(),
                target_qualified: module,
                file_path: self.file_path.clone(),
                line: node.start_position().row + 1,
            });
        }

        if self.is_class_node(kind)
            && let Some(name) = self.child_name(node)
        {
            let qn = qualify(&self.file_path, &name, None);
            self.nodes.push(StructuralNode {
                kind: NodeKind::Class,
                name: name.clone(),
                qualified_name: qn.clone(),
                file_path: self.file_path.clone(),
                line_start: node.start_position().row + 1,
                line_end: node.end_position().row + 1,
                language: self.language.clone(),
                parent_name: None,
                is_test: false,
            });
            self.edges.push(StructuralEdge {
                kind: EdgeKind::Contains,
                source_qualified: file_qn.to_string(),
                target_qualified: qn.clone(),
                file_path: self.file_path.clone(),
                line: node.start_position().row + 1,
            });
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk(child, Some(&name), enclosing_func_qn, file_qn);
            }
            return;
        }

        if self.is_function_node(kind)
            && let Some(name) = self.child_name(node)
        {
            let is_test = is_test_function(&name, &self.file_path);
            let qn = qualify(&self.file_path, &name, enclosing_class);
            self.nodes.push(StructuralNode {
                kind: if is_test {
                    NodeKind::Test
                } else {
                    NodeKind::Function
                },
                name: name.clone(),
                qualified_name: qn.clone(),
                file_path: self.file_path.clone(),
                line_start: node.start_position().row + 1,
                line_end: node.end_position().row + 1,
                language: self.language.clone(),
                parent_name: enclosing_class.map(|s| s.to_string()),
                is_test,
            });
            self.edges.push(StructuralEdge {
                kind: EdgeKind::Contains,
                source_qualified: if let Some(cls) = enclosing_class {
                    qualify(&self.file_path, cls, None)
                } else {
                    file_qn.to_string()
                },
                target_qualified: qn.clone(),
                file_path: self.file_path.clone(),
                line: node.start_position().row + 1,
            });
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk(child, enclosing_class, Some(&qn), file_qn);
            }
            // Dart: the body is the NEXT SIBLING (`function_body`), not a
            // child of the signature — walk it under this function's scope.
            if let Some(next) = node.next_sibling()
                && next.kind() == "function_body"
            {
                self.consumed_bodies.insert(next.id());
                let mut cursor = next.walk();
                for child in next.children(&mut cursor) {
                    self.walk(child, enclosing_class, Some(&qn), file_qn);
                }
            }
            return;
        }

        if self.is_call_node(kind)
            && let Some(target) = self.extract_call_target(node)
            && !target.is_empty()
        {
            let source = enclosing_func_qn.unwrap_or(file_qn).to_string();
            self.edges.push(StructuralEdge {
                kind: EdgeKind::Calls,
                source_qualified: source,
                target_qualified: target,
                file_path: self.file_path.clone(),
                line: node.start_position().row + 1,
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, enclosing_class, enclosing_func_qn, file_qn);
        }
    }
}

/// Resolve bare call targets (without `::`) to the matching qualified name
/// from the same file, leaving external calls untouched.
#[cfg(feature = "structural")]
fn resolve_call_targets(nodes: &[StructuralNode], edges: &mut [StructuralEdge]) {
    use std::collections::HashMap;
    let mut symbols: HashMap<String, String> = HashMap::new();
    for n in nodes {
        if matches!(
            n.kind,
            NodeKind::Function | NodeKind::Class | NodeKind::Test
        ) {
            symbols
                .entry(n.name.clone())
                .or_insert_with(|| n.qualified_name.clone());
        }
    }
    // Same-file typed receivers: `Foo::new` resolves to the node whose
    // parent_name is `Foo` and name is `new` in this file.
    let mut typed: HashMap<String, String> = HashMap::new();
    for n in nodes {
        if let Some(parent) = &n.parent_name
            && !parent.is_empty()
        {
            typed
                .entry(format!("{}::{}", parent, n.name))
                .or_insert_with(|| n.qualified_name.clone());
        }
    }

    for e in edges.iter_mut() {
        if !matches!(e.kind, EdgeKind::Calls) {
            continue;
        }
        if !e.target_qualified.contains("::") {
            if let Some(qn) = symbols.get(&e.target_qualified) {
                e.target_qualified = qn.clone();
            }
        } else if !e.target_qualified.contains('/')
            && !e.target_qualified.contains('.')
            && let Some(qn) = typed.get(&e.target_qualified)
        {
            e.target_qualified = qn.clone();
        }
    }
}

/// Parse a single file and return its structural nodes + edges. Returns
/// `None` for unsupported extensions or when the file can't be read.
#[cfg(feature = "structural")]
pub fn parse_file(file_path: &Path) -> Option<ParseResult> {
    parse_file_with_rel(file_path, None)
}

/// Parse a file using `rel_path` as the qualified-name prefix. Useful when
/// files live under a project root and paths should be stored as relative.
#[cfg(feature = "structural")]
pub fn parse_file_with_rel(file_path: &Path, rel_path: Option<&str>) -> Option<ParseResult> {
    let lang_name = langs::language_for(file_path)?;
    let language = langs::load_language(lang_name)?;
    // WSL UNC paths (`\\wsl$\…`) silently fail on `std::fs::read` from many
    // process contexts on Windows. `read_file_bytes` routes those through
    // `wsl.exe -- cat`. Non-WSL paths take the normal std::fs path.
    let source = crate::fs_util::read_file_bytes(file_path)?;

    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree: Tree = parser.parse(&source, None)?;

    let fp = rel_path
        .map(|s| s.to_string())
        .unwrap_or_else(|| file_path.to_string_lossy().replace('\\', "/"));

    let lang_walker = langs::for_language(lang_name)?;

    let mut walker = Walker {
        source: &source,
        file_path: fp.clone(),
        language: lang_name.to_string(),
        lang_walker,
        nodes: Vec::new(),
        edges: Vec::new(),
        consumed_bodies: std::collections::HashSet::new(),
    };

    let root = tree.root_node();
    let file_qn = fp.clone();

    // Always emit a File node so edges rooted at the file have a target.
    let last_line = source.iter().filter(|b| **b == b'\n').count() + 1;
    walker.nodes.push(StructuralNode {
        kind: NodeKind::File,
        name: fp.rsplit('/').next().unwrap_or(&fp).to_string(),
        qualified_name: file_qn.clone(),
        file_path: fp.clone(),
        line_start: 1,
        line_end: last_line,
        language: lang_name.to_string(),
        parent_name: None,
        is_test: is_test_function("", &fp),
    });

    walker.walk(root, None, None, &file_qn);

    let Walker {
        nodes, mut edges, ..
    } = walker;
    resolve_call_targets(&nodes, &mut edges);

    Some(ParseResult { nodes, edges })
}

/// Non-structural-feature fallback so callers can keep the signature.
#[cfg(not(feature = "structural"))]
pub fn parse_file(_file_path: &Path) -> Option<ParseResult> {
    None
}

#[cfg(not(feature = "structural"))]
pub fn parse_file_with_rel(_file_path: &Path, _rel_path: Option<&str>) -> Option<ParseResult> {
    None
}

#[cfg(all(test, feature = "structural"))]
mod tests {
    use super::*;

    fn write_tmp(name: &str, content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(name);
        std::fs::write(&p, content).unwrap();
        (tmp, p)
    }

    #[test]
    fn test_parse_rust_file() {
        let (_t, p) = write_tmp(
            "sample.rs",
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\nfn main() { add(1, 2); }\n",
        );
        let res = parse_file(&p).expect("rust parse");
        assert!(
            res.nodes.iter().any(|n| matches!(n.kind, NodeKind::File)),
            "file node missing"
        );
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Function) && n.name == "add"),
            "add function missing, got {:?}",
            res.nodes
                .iter()
                .map(|n| (n.kind, n.name.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            res.edges
                .iter()
                .any(|e| matches!(e.kind, EdgeKind::Calls) && e.target_qualified.ends_with("::add")),
            "call edge to add missing, got {:?}",
            res.edges
                .iter()
                .map(|e| (e.kind.as_str(), e.target_qualified.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_parse_python_class_and_method() {
        let (_t, p) = write_tmp(
            "svc.py",
            "class Service:\n    def handle(self):\n        return self.helper()\n    def helper(self):\n        return 1\n",
        );
        let res = parse_file(&p).expect("python parse");
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Class) && n.name == "Service"),
            "class missing"
        );
        let method = res
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Function) && n.name == "handle")
            .expect("handle method missing");
        assert_eq!(method.parent_name.as_deref(), Some("Service"));
        assert!(method.qualified_name.contains("Service::handle"));
    }

    #[test]
    fn test_parse_typescript_imports() {
        let (_t, p) = write_tmp(
            "app.ts",
            "import { helper } from './helper';\nfunction run() { helper(); }\n",
        );
        let res = parse_file(&p).expect("ts parse");
        assert!(
            res.edges
                .iter()
                .any(|e| matches!(e.kind, EdgeKind::ImportsFrom)),
            "import edge missing"
        );
    }

    #[test]
    fn test_parse_dart_class_and_method() {
        let (_t, p) = write_tmp(
            "widget.dart",
            "class MyWidget {\n  void build() {}\n  String title() => 'hi';\n}\n",
        );
        let res = parse_file(&p).expect("dart parse");
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Class) && n.name == "MyWidget"),
            "MyWidget class missing, got {:?}",
            res.nodes
                .iter()
                .map(|n| (n.kind, n.name.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_parse_java_class_and_method() {
        let (_t, p) = write_tmp(
            "Svc.java",
            "package a;\npublic class Svc {\n    public void run() {}\n    public int add(int a, int b) { return a + b; }\n}\n",
        );
        let res = parse_file(&p).expect("java parse");
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Class) && n.name == "Svc"),
            "Svc class missing"
        );
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Function) && n.name == "add"),
            "add method missing, got {:?}",
            res.nodes
                .iter()
                .map(|n| (n.kind, n.name.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_parse_php_function() {
        let (_t, p) = write_tmp(
            "app.php",
            "<?php\nfunction helper() { return 1; }\nfunction run() { return helper(); }\n",
        );
        let res = parse_file(&p).expect("php parse");
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Function) && n.name == "helper"),
            "helper function missing"
        );
        assert!(
            res.edges
                .iter()
                .any(|e| matches!(e.kind, EdgeKind::Calls)
                    && e.target_qualified.ends_with("::helper")),
            "call edge to helper missing"
        );
    }

    #[test]
    fn test_parse_c_function() {
        let (_t, p) = write_tmp(
            "calc.c",
            "#include <stdio.h>\nint add(int a, int b) { return a + b; }\nint main() { return add(1, 2); }\n",
        );
        let res = parse_file(&p).expect("c parse");
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Function) && n.name == "add"),
            "add function missing, got {:?}",
            res.nodes
                .iter()
                .map(|n| (n.kind, n.name.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            res.edges
                .iter()
                .any(|e| matches!(e.kind, EdgeKind::ImportsFrom)),
            "#include should emit an IMPORTS_FROM edge"
        );
    }

    #[test]
    fn test_parse_cpp_class_and_method() {
        let (_t, p) = write_tmp(
            "svc.cpp",
            "class Svc {\npublic:\n    int run() { return 1; }\n};\nint main() { Svc s; return s.run(); }\n",
        );
        let res = parse_file(&p).expect("cpp parse");
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Class) && n.name == "Svc"),
            "Svc class missing"
        );
        assert!(
            res.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Function)),
            "no function parsed in cpp, got {:?}",
            res.nodes
                .iter()
                .map(|n| (n.kind, n.name.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_parse_unsupported_returns_none() {
        let (_t, p) = write_tmp("data.xyz", "not parseable");
        assert!(parse_file(&p).is_none());
    }

    #[test]
    fn test_parse_elixir_module_and_function() {
        // tree-sitter-elixir is a minimal grammar: defmodule/def/defp
        // all parse as `call` nodes. Our walker still emits the File
        // node and turns every `call` into a Calls edge — enough to
        // confirm Elixir files parse and contribute to the graph.
        let (_t, p) = write_tmp(
            "auth.ex",
            "defmodule MyApp.Auth do\n  def login(user), do: :ok\n  defp valid?(token), do: true\nend\n",
        );
        let res = parse_file(&p).expect("elixir parse");
        // File node always present — primary acceptance criterion.
        assert!(
            res.nodes.iter().any(|n| matches!(n.kind, NodeKind::File)),
            "expected File node, got {:?}",
            res.nodes
                .iter()
                .map(|n| (n.kind, n.name.clone()))
                .collect::<Vec<_>>()
        );
        // The defmodule + def + defp + do: pairs all parse as calls →
        // the walker emits at least one Calls edge.
        assert!(
            res.edges.iter().any(|e| matches!(e.kind, EdgeKind::Calls)),
            "expected at least one Calls edge, got {:?}",
            res.edges.iter().map(|e| e.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_is_test_function_heuristics() {
        assert!(is_test_function("test_foo", "src/lib.rs"));
        assert!(is_test_function("handler", "tests/integration.rs"));
        assert!(is_test_function("ok", "something_test.go"));
        assert!(!is_test_function("handler", "src/lib.rs"));
    }
}
