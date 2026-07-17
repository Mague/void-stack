//! Verse (UEFN / Unreal Editor for Fortnite) import parser.
//!
//! Parses Verse `using` directives:
//! ```text
//! using { /Fortnite.com/Devices }
//! using { /Verse.org/Simulation }
//! using { MyModule }
//! ```
//!
//! Absolute module paths (starting with `/`) reference the Verse digest
//! (Fortnite.com, Verse.org, UnrealEngine.com) and are treated as external;
//! bare module names are project-local (relative) references.

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct VerseParser;

impl ImportParser for VerseParser {
    fn language(&self) -> Language {
        Language::Verse
    }

    fn file_extensions(&self) -> &[&str] {
        &[".verse"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
        let mut imports = Vec::new();
        let mut class_count = 0;
        let mut function_count = 0;
        let mut loc = 0;

        for line in content.lines() {
            let trimmed = line.trim();

            // Verse comments start with `#` (block comments `<# #>` also
            // start with a leading `#` on their opening line).
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            loc += 1;

            // using { /Fortnite.com/Devices } / using { MyModule }
            if trimmed.starts_with("using")
                && let (Some(open), Some(close)) = (trimmed.find('{'), trimmed.rfind('}'))
                && open < close
            {
                let module = trimmed[open + 1..close].trim();
                if !module.is_empty() {
                    imports.push(RawImport {
                        module_path: module.to_string(),
                        is_relative: !module.starts_with('/'),
                    });
                }
                continue;
            }

            // Class definitions: `my_device := class(creative_device):`
            if trimmed.contains(":= class") {
                class_count += 1;
                continue;
            }

            // Function definitions: `MyFunc(X : int) : int = ...` and
            // device events like `OnBegin<override>()<suspends> : void =`.
            if is_verse_function(trimmed) {
                function_count += 1;
            }
        }

        ParseResult {
            imports,
            class_count,
            function_count,
            loc,
            is_hub: false,
            has_framework_macros: false,
        }
    }
}

/// Pragmatic Verse function-definition heuristic: an identifier (optionally
/// carrying `<specifiers>`) directly followed by a parameter list, then a
/// `:` return type and an `=` after the closing parenthesis.
fn is_verse_function(line: &str) -> bool {
    // Must start with an identifier character.
    if !line
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        return false;
    }

    let open = match line.find('(') {
        Some(i) => i,
        None => return false,
    };

    // Everything before `(` must be identifier chars plus optional
    // `<override>` / `<public>` style specifiers — no spaces, so control
    // flow like `if (X):` never matches.
    let head = &line[..open];
    if !head
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '<' || c == '>')
    {
        return false;
    }

    // Find the parenthesis closing the parameter list (depth-matched, so
    // default values with calls inside don't confuse us).
    let mut depth = 0i32;
    let mut close = None;
    for (i, c) in line.char_indices().skip(open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = match close {
        Some(i) => i,
        None => return false,
    };

    // After the parameter list: optional `<suspends>` specifiers, then
    // `: type` and an `=` (body on the same line or an indented block).
    let tail = &line[close + 1..];
    match tail.find(':') {
        Some(colon) => tail[colon + 1..].contains('='),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verse_using_absolute_is_external() {
        let parser = VerseParser;
        let content = "\
using { /Fortnite.com/Devices }
using { /Verse.org/Simulation }
using { /UnrealEngine.com/Temporary/SpatialMath }
";
        let result = parser.parse_file(content, "device.verse");
        assert_eq!(result.imports.len(), 3);
        assert_eq!(result.imports[0].module_path, "/Fortnite.com/Devices");
        assert!(
            result.imports.iter().all(|i| !i.is_relative),
            "digest paths starting with '/' must be treated as absolute/external"
        );
    }

    #[test]
    fn test_verse_using_bare_module_is_relative() {
        let parser = VerseParser;
        let content = "using { MyModule }\nusing {  Helpers  }\n";
        let result = parser.parse_file(content, "game.verse");
        assert_eq!(result.imports.len(), 2);
        assert_eq!(result.imports[0].module_path, "MyModule");
        assert_eq!(
            result.imports[1].module_path, "Helpers",
            "module path must be trimmed inside the braces"
        );
        assert!(
            result.imports.iter().all(|i| i.is_relative),
            "bare module names must be relative (project-local)"
        );
    }

    #[test]
    fn test_verse_class_and_function_counting() {
        let parser = VerseParser;
        let content = "\
using { /Fortnite.com/Devices }

my_device := class(creative_device):

    OnBegin<override>()<suspends> : void =
        Print(\"Hello\")

    Add(X : int, Y : int) : int =
        X + Y

game_manager := class:
    Score : int = 0
";
        let result = parser.parse_file(content, "my_device.verse");
        assert_eq!(result.class_count, 2, "my_device + game_manager");
        assert_eq!(
            result.function_count, 2,
            "OnBegin and Add should count as functions"
        );
    }

    #[test]
    fn test_verse_comments_and_blank_lines_excluded_from_loc() {
        let parser = VerseParser;
        let content = "\
# This is a comment
using { MyModule }

# TODO: another comment
Fn() : void =
    Print(\"x\")
";
        let result = parser.parse_file(content, "x.verse");
        // Counted: using, Fn signature, Print body = 3 lines.
        assert_eq!(
            result.loc, 3,
            "comments and blank lines must not count as LOC"
        );
        assert_eq!(result.imports.len(), 1);
    }

    #[test]
    fn test_verse_control_flow_not_counted_as_function() {
        let parser = VerseParser;
        let content = "\
Run(N : int) : void =
    if (N > 0):
        Print(\"positive\")
    loop:
        DoWork()
";
        let result = parser.parse_file(content, "run.verse");
        assert_eq!(
            result.function_count, 1,
            "only Run is a function; if/loop/calls are not"
        );
    }

    #[test]
    fn test_is_verse_function_shapes() {
        // Plain function with return type and inline body marker.
        assert!(is_verse_function("MyFunc(X : int) : int = X * 2"));
        // Device event with specifiers.
        assert!(is_verse_function("OnBegin<override>()<suspends> : void ="));
        // Missing `=` after the return type is a declaration, not a definition.
        assert!(!is_verse_function("MyFunc(X : int) : int"));
        // Plain call.
        assert!(!is_verse_function("Print(\"hi\")"));
        // Control flow.
        assert!(!is_verse_function("if (Score > 10):"));
        // Assignment via :=.
        assert!(!is_verse_function("X := Foo(1)"));
    }
}
