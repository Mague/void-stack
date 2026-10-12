//! C-family include parsers: C / C++ (Unreal Engine C++ modules, plain
//! C/C++) and MQL4 / MQL5 (MetaTrader).
//!
//! `#include "Foo.h"` / `#include "Components/Health.h"` are project
//! includes: they resolve against the including file's folder or, as
//! Unreal resolves them, against any module folder (`Public/`, `Classes/`,
//! `Private/`) — matched by path suffix in `build_graph`. Unresolved
//! quoted includes (`CoreMinimal.h`, `GameFramework/Actor.h`) are engine
//! headers and count as external; `#include <...>` is always external.

use crate::analyzer::graph::Language;
use crate::analyzer::imports::{ImportParser, ParseResult, RawImport};

pub struct CppParser;

impl ImportParser for CppParser {
    fn language(&self) -> Language {
        Language::Cpp
    }

    fn file_extensions(&self) -> &[&str] {
        &[".cpp", ".cc", ".cxx", ".c", ".h", ".hpp"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
        parse_c_like(content)
    }
}

/// Includes, LOC and type / function counts for C-family sources. Shared
/// with MQL, whose preprocessor and declarations follow C++.
fn parse_c_like(content: &str) -> ParseResult {
    let mut out = ParseResult::default();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }
        out.loc += 1;
        if let Some(rest) = t.strip_prefix("#include") {
            let rest = rest.trim();
            if let Some(path) = rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
                out.imports.push(RawImport {
                    module_path: path.to_string(),
                    is_relative: false,
                });
            } else if let Some(path) = rest.strip_prefix('<').and_then(|r| r.split('>').next()) {
                out.imports.push(RawImport {
                    module_path: format!("<{}>", path),
                    is_relative: false,
                });
            }
            continue;
        }
        if declares_type(t) {
            out.class_count += 1;
        } else if crate::analyzer::complexity::signatures::cpp_function_name(t).is_some() {
            out.function_count += 1;
        }
    }
    out
}

/// `class FOO_API AMyActor : public AActor` / `struct FData {` — a type
/// *definition*, not a forward declaration (`class AActor;`).
fn declares_type(t: &str) -> bool {
    (t.starts_with("class ") || t.starts_with("struct ")) && !t.ends_with(';')
}

/// MQL4 / MQL5 (MetaTrader experts, indicators, scripts, `.mqh`
/// libraries). The preprocessor follows C++: `#include "Utils.mqh"`
/// resolves next to the including file, `#include <Trade\Trade.mqh>`
/// against the terminal's `Include/` folder (see `resolve_mql_include`).
pub struct MqlParser;

impl ImportParser for MqlParser {
    fn language(&self) -> Language {
        Language::Mql
    }

    fn file_extensions(&self) -> &[&str] {
        &[".mq4", ".mq5", ".mqh"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
        let mut out = parse_c_like(content);
        // MetaEditor writes Windows separators in include paths.
        for imp in &mut out.imports {
            imp.module_path = imp.module_path.replace('\\', "/");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_includes_and_counts() {
        let src = "#include \"MyActor.h\"\n#include \"Components/HealthComponent.h\"\n#include <vector>\nclass AActor;\nclass MYGAME_API AMyActor : public AActor\n{\n};\nvoid AMyActor::BeginPlay()\n{\n\tSuper::BeginPlay();\n}\n";
        let r = CppParser.parse_file(src, "Source/MyGame/Private/MyActor.cpp");
        let imports: Vec<&str> = r.imports.iter().map(|i| i.module_path.as_str()).collect();
        assert_eq!(
            imports,
            vec!["MyActor.h", "Components/HealthComponent.h", "<vector>"]
        );
        assert_eq!(r.class_count, 1, "forward declarations don't count");
        assert_eq!(r.function_count, 1);
    }

    #[test]
    fn mql_parses_includes_and_event_handlers() {
        let src = "#property strict\n#include <Trade\\Trade.mqh>\n#include \"Utils.mqh\"\ninput double Lots = 0.1;\nint OnInit()\n{\n   return(INIT_SUCCEEDED);\n}\nvoid OnTick()\n{\n   if(IsNewBar()) Trade();\n}\n";
        let r = MqlParser.parse_file(src, "Experts/Bot.mq4");
        let imports: Vec<&str> = r.imports.iter().map(|i| i.module_path.as_str()).collect();
        assert_eq!(imports, vec!["<Trade/Trade.mqh>", "Utils.mqh"]);
        assert_eq!(r.function_count, 2, "OnInit + OnTick, not the input");
    }
}
