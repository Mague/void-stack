//! C# import parser (Unity, Godot .NET, plain .NET).
//!
//! C# files depend on *namespaces*, not files: `using Game.Core;` pulls in
//! every type declared under `namespace Game.Core`. The parser reports the
//! `using` targets as imports and the file's own `namespace` declarations
//! in [`ParseResult::declares`]; `build_graph` links each import to the
//! project files that declare that namespace. Namespaces nobody in the
//! project declares (`UnityEngine`, `System.Collections`) are external.

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct CSharpParser;

impl ImportParser for CSharpParser {
    fn language(&self) -> Language {
        Language::CSharp
    }

    fn file_extensions(&self) -> &[&str] {
        &[".cs"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
        let mut out = ParseResult::default();
        let mut ids = std::collections::HashSet::new();
        let mut in_block_comment = false;

        for line in content.lines() {
            let mut t = line.trim();
            if in_block_comment {
                match t.find("*/") {
                    Some(end) => {
                        in_block_comment = false;
                        t = t[end + 2..].trim();
                    }
                    None => continue,
                }
            }
            if t.starts_with("/*") && !t.contains("*/") {
                in_block_comment = true;
                continue;
            }
            if t.is_empty() || t.starts_with("//") {
                continue;
            }
            out.loc += 1;

            if let Some(ns) = using_target(t) {
                out.imports.push(RawImport {
                    module_path: ns,
                    is_relative: false,
                });
                continue;
            }
            if let Some(rest) = t.strip_prefix("namespace ") {
                let ns = rest.trim_end_matches(['{', ';']).trim().to_string();
                if !ns.is_empty() {
                    // Code in `A.B` sees the types of `A.B` and `A` unqualified.
                    let mut scope = ns.as_str();
                    loop {
                        out.implicit_imports.push(scope.to_string());
                        match scope.rsplit_once('.') {
                            Some((parent, _)) => scope = parent,
                            None => break,
                        }
                    }
                    out.declares.push(ns);
                }
                continue;
            }
            // `using`/`namespace` lines name namespaces, not types.
            super::collect_identifiers(t, &mut ids);
            if let Some(name) = declared_type_name(t) {
                out.class_count += 1;
                out.type_names.push(name);
            } else if crate::analyzer::signatures::csharp_method_name(t).is_some() {
                out.function_count += 1;
            }
        }
        out.identifiers = Some(ids);
        out
    }
}

/// `using X.Y;`, `global using X.Y;`, `using Alias = X.Y;` → `X.Y`.
/// `using static X.Y;` imports members of a type — reported as `X.Y`'s
/// namespace-qualified type, which won't match a namespace and stays
/// external unless the project declares it. `using (var x = ...)`
/// statements are not directives.
fn using_target(t: &str) -> Option<String> {
    let t = t.strip_prefix("global ").unwrap_or(t);
    let rest = t.strip_prefix("using ")?;
    if rest.starts_with('(') || rest.starts_with("var ") || !rest.ends_with(';') {
        return None;
    }
    let rest = rest.trim_end_matches(';').trim();
    let rest = rest.strip_prefix("static ").unwrap_or(rest);
    let target = match rest.split_once('=') {
        Some((_, rhs)) => rhs.trim(),
        None => rest,
    };
    (!target.is_empty()).then(|| target.to_string())
}

/// The type a line declares (`public sealed class Player : Mono` →
/// `Player`), or `None`.
fn declared_type_name(t: &str) -> Option<String> {
    const KINDS: &[&str] = &["class", "struct", "interface", "enum", "record"];
    if t.ends_with(';') {
        return None; // forward-ish one-liners: `record R(int X);`
    }
    // Only tokens before the first `(` / `{` can name the declaration kind
    // (`interface IHit { void Hit(); }` is a type, `void Run(class c)` isn't).
    let mut tokens = t.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok.contains('(') || tok.contains('{') {
            return None;
        }
        if KINDS.contains(&tok) {
            return tokens.next().and_then(super::type_name_token);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usings_namespaces_and_counts() {
        let src = "using UnityEngine;\nusing Game.Core;\nusing static Game.Util.MathX;\nusing Vec = UnityEngine.Vector3;\nnamespace Game.Player {\n  public class PlayerController : MonoBehaviour {\n    void Start() { using (var s = Open()) {} }\n    public int Hp() { return 1; }\n  }\n  interface IHit { void Hit(); }\n}\n";
        let r = CSharpParser.parse_file(src, "Assets/Player.cs");
        let imports: Vec<&str> = r.imports.iter().map(|i| i.module_path.as_str()).collect();
        assert_eq!(
            imports,
            vec![
                "UnityEngine",
                "Game.Core",
                "Game.Util.MathX",
                "UnityEngine.Vector3"
            ]
        );
        assert_eq!(r.declares, vec!["Game.Player".to_string()]);
        assert_eq!(r.class_count, 2);
        assert_eq!(r.function_count, 2);
    }

    #[test]
    fn file_scoped_namespace() {
        let r = CSharpParser.parse_file(
            "namespace Game.Core;\npublic class Health {}\n",
            "Health.cs",
        );
        assert_eq!(r.declares, vec!["Game.Core".to_string()]);
    }
}
