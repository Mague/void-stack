//! GDScript import parser (Godot).
//!
//! GDScript files depend on each other through resource paths —
//! `extends "res://actors/base.gd"`, `preload("res://x.gd")`,
//! `load("res://x.gd")` — and through global class names registered with
//! `class_name Player` (then `extends Player` / `Player.new()`).
//! `res://` paths resolve against the project root; `class_name`
//! declarations are reported in [`ParseResult::declares`] so `build_graph`
//! can link `extends Player` to the file that declares it. Engine classes
//! (`Node2D`, `CharacterBody2D`) don't resolve and are not reported as
//! external packages.

use crate::analyzer::graph::Language;
use crate::analyzer::imports::{ImportParser, ParseResult, RawImport};

pub struct GdScriptParser;

impl ImportParser for GdScriptParser {
    fn language(&self) -> Language {
        Language::GdScript
    }

    fn file_extensions(&self) -> &[&str] {
        &[".gd"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
        let mut out = ParseResult::default();
        for line in content.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            out.loc += 1;

            if let Some(name) = t.strip_prefix("class_name ") {
                let name = name.split([' ', ',', '#']).next().unwrap_or("").trim();
                if !name.is_empty() {
                    out.declares.push(name.to_string());
                }
                continue;
            }
            if let Some(target) = t.strip_prefix("extends ") {
                let target = target.split('#').next().unwrap_or("").trim();
                if let Some(path) = res_path(target) {
                    out.imports.push(RawImport {
                        module_path: path,
                        is_relative: false,
                    });
                } else if !target.is_empty() {
                    // A class name: project `class_name` or an engine class.
                    // Marked relative so unresolved engine classes are not
                    // listed as external packages.
                    out.imports.push(RawImport {
                        module_path: target.to_string(),
                        is_relative: true,
                    });
                }
                continue;
            }
            for path in load_paths(t) {
                out.imports.push(RawImport {
                    module_path: path,
                    is_relative: false,
                });
            }
            if t.starts_with("class ") && t.ends_with(':') {
                out.class_count += 1;
            } else if crate::analyzer::complexity::signatures::gdscript_function_name(t).is_some() {
                out.function_count += 1;
            }
        }
        out
    }
}

/// `"res://a/b.gd"` → `a/b.gd`.
fn res_path(s: &str) -> Option<String> {
    let inner = s.trim().trim_matches(|c| c == '"' || c == '\'');
    inner
        .strip_prefix("res://")
        .filter(|p| !p.is_empty())
        .map(str::to_string)
}

/// Every `preload("res://…")` / `load("res://…")` path on the line.
fn load_paths(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["preload(", "load("] {
        let mut rest = line;
        while let Some(i) = rest.find(key) {
            // `preload(` also contains `load(`: skip that overlap.
            let is_overlap = key == "load(" && rest[..i].ends_with("pre");
            let after = &rest[i + key.len()..];
            if !is_overlap
                && let Some(end) = after.find(')')
                && let Some(p) = res_path(&after[..end])
                && !out.contains(&p)
            {
                out.push(p);
            }
            rest = after;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extends_loads_and_class_name() {
        let src = "extends \"res://actors/base.gd\"\nclass_name Player\nconst Bullet = preload(\"res://weapons/bullet.gd\")\nvar fx = load(\"res://fx/spark.gd\")\nvar tex = preload(\"res://art/p.png\")\nfunc _ready():\n\tpass\nclass Inner:\n\tfunc f():\n\t\tpass\n";
        let r = GdScriptParser.parse_file(src, "actors/player.gd");
        let imports: Vec<&str> = r.imports.iter().map(|i| i.module_path.as_str()).collect();
        assert_eq!(
            imports,
            vec![
                "actors/base.gd",
                "weapons/bullet.gd",
                "fx/spark.gd",
                "art/p.png"
            ]
        );
        assert_eq!(r.declares, vec!["Player".to_string()]);
        assert_eq!(r.function_count, 2);
        assert_eq!(r.class_count, 1);
    }

    #[test]
    fn extends_class_name_is_symbolic() {
        let r = GdScriptParser.parse_file("extends Enemy\n", "boss.gd");
        assert_eq!(r.imports.len(), 1);
        assert_eq!(r.imports[0].module_path, "Enemy");
        assert!(r.imports[0].is_relative);
    }
}
