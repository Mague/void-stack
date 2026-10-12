//! C / C++ include parser (Unreal Engine C++ modules, plain C/C++).
//!
//! `#include "Foo.h"` / `#include "Components/Health.h"` are project
//! includes: they resolve against the including file's folder or, as
//! Unreal resolves them, against any module folder (`Public/`, `Classes/`,
//! `Private/`) — matched by path suffix in `build_graph`. Unresolved
//! quoted includes (`CoreMinimal.h`, `GameFramework/Actor.h`) are engine
//! headers and count as external; `#include <...>` is always external.

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct CppParser;

impl ImportParser for CppParser {
    fn language(&self) -> Language {
        Language::Cpp
    }

    fn file_extensions(&self) -> &[&str] {
        &[".cpp", ".cc", ".cxx", ".c", ".h", ".hpp"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
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
                } else if let Some(path) = rest.strip_prefix('<').and_then(|r| r.split('>').next())
                {
                    out.imports.push(RawImport {
                        module_path: format!("<{}>", path),
                        is_relative: false,
                    });
                }
                continue;
            }
            if declares_type(t) {
                out.class_count += 1;
            } else if crate::analyzer::signatures::cpp_function_name(t).is_some() {
                out.function_count += 1;
            }
        }
        out
    }
}

/// `class FOO_API AMyActor : public AActor` / `struct FData {` — a type
/// *definition*, not a forward declaration (`class AActor;`).
fn declares_type(t: &str) -> bool {
    (t.starts_with("class ") || t.starts_with("struct ")) && !t.ends_with(';')
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
}
