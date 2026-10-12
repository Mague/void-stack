//! Swift import parser (SwiftPM / Xcode).
//!
//! Swift imports *modules*: `import MyKit` makes every public symbol of the
//! `MyKit` module visible. In a SwiftPM package each target's sources live
//! in `Sources/<Target>/` (tests in `Tests/<Target>/`), so a file declares
//! the module named after that folder; `build_graph` links imports to the
//! files of the matching module. System frameworks (`Foundation`,
//! `SwiftUI`, `UIKit`) are external.

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct SwiftParser;

impl ImportParser for SwiftParser {
    fn language(&self) -> Language {
        Language::Swift
    }

    fn file_extensions(&self) -> &[&str] {
        &[".swift"]
    }

    fn parse_file(&self, content: &str, file_path: &str) -> ParseResult {
        let mut out = ParseResult::default();
        let mut ids = std::collections::HashSet::new();
        if let Some(module) = spm_module(file_path) {
            // Files of one module see each other without importing it.
            out.implicit_imports.push(module.clone());
            out.declares.push(module);
        }
        for line in content.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
                continue;
            }
            out.loc += 1;
            if let Some(module) = import_target(t) {
                out.imports.push(RawImport {
                    module_path: module,
                    is_relative: false,
                });
                continue;
            }
            super::collect_identifiers(t, &mut ids);
            if let Some(name) = declared_type(t) {
                out.class_count += 1;
                // An extension adds to a type declared elsewhere.
                if let Some(name) = name {
                    out.type_names.push(name);
                }
            } else if crate::analyzer::signatures::swift_function_name(t).is_some() {
                out.function_count += 1;
            }
        }
        out.identifiers = Some(ids);
        out
    }
}

/// `import Foo`, `@testable import Foo`, `import struct Foo.Bar`,
/// `@_exported import Foo` → `Foo` (the module).
fn import_target(t: &str) -> Option<String> {
    let mut tokens = t.split_whitespace().skip_while(|tok| tok.starts_with('@'));
    if tokens.next()? != "import" {
        return None;
    }
    let mut next = tokens.next()?;
    const KINDS: &[&str] = &[
        "struct",
        "class",
        "enum",
        "protocol",
        "typealias",
        "func",
        "let",
        "var",
    ];
    if KINDS.contains(&next) {
        next = tokens.next()?;
    }
    let module = next.split('.').next()?.trim();
    (!module.is_empty()).then(|| module.to_string())
}

/// `Sources/<Module>/...` or `Tests/<Module>/...` → `<Module>`.
fn spm_module(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    parts
        .windows(2)
        .find(|w| w[0] == "Sources" || w[0] == "Tests")
        .map(|w| w[1])
        .filter(|m| !m.ends_with(".swift"))
        .map(str::to_string)
}

/// `Some(Some(name))` for a type declaration, `Some(None)` for an
/// `extension`, `None` for anything else.
fn declared_type(t: &str) -> Option<Option<String>> {
    const KINDS: &[&str] = &["class", "struct", "enum", "protocol", "actor", "extension"];
    let mut tokens = t
        .split_whitespace()
        .skip_while(|tok| tok.starts_with('@') || is_modifier(tok));
    // `class func` / `class var` are type members, not a class declaration.
    match (tokens.next(), tokens.next()) {
        (Some(kind), next) if KINDS.contains(&kind) => {
            if matches!(next, Some("func") | Some("var") | Some("let")) {
                return None;
            }
            Some(
                (kind != "extension")
                    .then(|| next.and_then(super::type_name_token))
                    .flatten(),
            )
        }
        _ => None,
    }
}

fn is_modifier(tok: &str) -> bool {
    matches!(
        tok,
        "public" | "private" | "fileprivate" | "internal" | "open" | "final" | "indirect"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_imports_module_and_counts() {
        let src = "import Foundation\n@testable import GameKit\nimport struct Physics.Vector\n\npublic final class Game {\n  func start() {}\n}\nstruct Pt {}\nextension Pt {\n  func len() -> Int { 0 }\n}\n";
        let r = SwiftParser.parse_file(src, "Sources/GameApp/Game.swift");
        let imports: Vec<&str> = r.imports.iter().map(|i| i.module_path.as_str()).collect();
        assert_eq!(imports, vec!["Foundation", "GameKit", "Physics"]);
        assert_eq!(r.declares, vec!["GameApp".to_string()]);
        assert_eq!(r.class_count, 3);
        assert_eq!(r.function_count, 2);
    }

    #[test]
    fn xcode_layout_declares_nothing() {
        let r = SwiftParser.parse_file("import UIKit\n", "App/ViewController.swift");
        assert!(r.declares.is_empty());
    }
}
