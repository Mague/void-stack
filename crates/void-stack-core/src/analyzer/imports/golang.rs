//! Go import parser.
//!
//! Parses single and grouped imports:
//! ```text
//! import "fmt"
//! import (
//!     "net/http"
//!     "github.com/user/pkg/handler"
//! )
//! ```

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct GoParser;

impl ImportParser for GoParser {
    fn language(&self) -> Language {
        Language::Go
    }

    fn file_extensions(&self) -> &[&str] {
        &[".go"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
        let mut imports = Vec::new();
        let mut class_count = 0; // structs
        let mut function_count = 0;
        let mut loc = 0;

        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;
        let mut in_import_block = false;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            if trimmed.is_empty() || trimmed.starts_with("//") {
                i += 1;
                continue;
            }
            loc += 1;

            // Single import: import "fmt"
            if trimmed.starts_with("import \"") || trimmed.starts_with("import `") {
                if let Some(path) = extract_go_string(trimmed.strip_prefix("import ").unwrap_or("")) {
                    imports.push(make_go_import(&path));
                }
            }

            // Grouped import block: import ( ... )
            if trimmed == "import (" {
                in_import_block = true;
                i += 1;
                continue;
            }
            if in_import_block {
                if trimmed == ")" {
                    in_import_block = false;
                } else {
                    // May have alias: alias "path" or just "path"
                    let part = trimmed.split_whitespace().last().unwrap_or(trimmed);
                    if let Some(path) = extract_go_string(part) {
                        imports.push(make_go_import(&path));
                    }
                }
                i += 1;
                continue;
            }

            // Count structs (type X struct)
            if trimmed.starts_with("type ") && trimmed.contains(" struct") {
                class_count += 1;
            }

            // Count interfaces as classes too
            if trimmed.starts_with("type ") && trimmed.contains(" interface") {
                class_count += 1;
            }

            // Count functions: func Name( or func (receiver) Name(
            if trimmed.starts_with("func ") && trimmed.contains('(') {
                function_count += 1;
            }

            i += 1;
        }

        ParseResult {
            imports,
            class_count,
            function_count,
            loc,
        }
    }
}

fn extract_go_string(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.starts_with('"') && trimmed.len() > 1 {
        let end = trimmed[1..].find('"')?;
        return Some(trimmed[1..1 + end].to_string());
    }
    if trimmed.starts_with('`') && trimmed.len() > 1 {
        let end = trimmed[1..].find('`')?;
        return Some(trimmed[1..1 + end].to_string());
    }
    None
}

fn make_go_import(path: &str) -> RawImport {
    // Standard library imports don't contain dots in the first segment
    let first_segment = path.split('/').next().unwrap_or(path);
    let _is_external = first_segment.contains('.');

    RawImport {
        module_path: path.to_string(),
        is_relative: false, // Go doesn't have relative imports
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_imports() {
        let parser = GoParser;
        let content = r#"
package main

import "fmt"
import "net/http"

func main() {
    fmt.Println("hello")
}
"#;
        let result = parser.parse_file(content, "main.go");
        assert_eq!(result.imports.len(), 2);
        assert_eq!(result.imports[0].module_path, "fmt");
        assert_eq!(result.imports[1].module_path, "net/http");
        assert_eq!(result.function_count, 1);
    }

    #[test]
    fn test_grouped_imports() {
        let parser = GoParser;
        let content = r#"
package handler

import (
    "encoding/json"
    "net/http"

    "github.com/user/pkg/models"
    "github.com/user/pkg/services"
)

type Handler struct {
    svc *services.Service
}

type Config interface {
    Get(key string) string
}

func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
}

func NewHandler(svc *services.Service) *Handler {
    return &Handler{svc: svc}
}
"#;
        let result = parser.parse_file(content, "handler.go");
        assert_eq!(result.imports.len(), 4);
        assert_eq!(result.class_count, 2); // Handler struct + Config interface
        assert_eq!(result.function_count, 2); // ServeHTTP + NewHandler
    }

    #[test]
    fn test_aliased_imports() {
        let parser = GoParser;
        let content = r#"
package main

import (
    . "fmt"
    _ "net/http/pprof"
    mux "github.com/gorilla/mux"
)
"#;
        let result = parser.parse_file(content, "main.go");
        assert_eq!(result.imports.len(), 3);
        assert_eq!(result.imports[0].module_path, "fmt");
        assert_eq!(result.imports[2].module_path, "github.com/gorilla/mux");
    }
}
