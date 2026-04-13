//! Rust import parser.
//!
//! Parses use declarations, mod declarations, and extern crate:
//! ```text
//! use std::collections::HashMap;
//! use crate::models::User;
//! use super::service;
//! mod handler;
//! extern crate serde;
//! ```

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct RustParser;

impl ImportParser for RustParser {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn file_extensions(&self) -> &[&str] {
        &[".rs"]
    }

    fn parse_file(&self, content: &str, file_path: &str) -> ParseResult {
        let mut imports = Vec::new();
        let mut class_count = 0; // structs + enums + traits
        let mut function_count = 0;
        let mut loc = 0;
        let mut in_block_comment = false;

        // Fix 1: skip everything inside `#[cfg(test)] mod … { … }` blocks so
        //        the test module doesn't inflate LOC / function_count.
        // Fix 2: count `pub use` / `pub mod` / bare `mod` lines to compute
        //        a hub score at the end.
        // Fix 3: presence of rmcp routing macros short-circuits the
        //        function_count god-class trigger (detected post-loop).
        let mut in_test_block = false;
        let mut test_block_depth: i32 = 0;
        let mut pending_test_attr = false;
        let mut reexport_lines = 0usize;
        let has_framework_macros =
            content.contains("#[tool_router]") || content.contains("#[tool_handler]");

        for line in content.lines() {
            let trimmed = line.trim();

            // Handle block comments
            if in_block_comment {
                if trimmed.contains("*/") {
                    in_block_comment = false;
                }
                continue;
            }
            if trimmed.starts_with("/*") {
                in_block_comment = true;
                if trimmed.contains("*/") {
                    in_block_comment = false;
                }
                continue;
            }

            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Fix 1 — test block tracking ────────────────────────────
            if in_test_block {
                test_block_depth += trimmed.chars().filter(|&c| c == '{').count() as i32;
                test_block_depth -= trimmed.chars().filter(|&c| c == '}').count() as i32;
                if test_block_depth <= 0 {
                    in_test_block = false;
                }
                continue; // don't count LOC or functions inside the test block
            }
            // `#[cfg(test)] mod tests {` on the same line opens immediately.
            if trimmed.starts_with("#[cfg(test)]")
                && trimmed.contains("mod ")
                && trimmed.ends_with('{')
            {
                in_test_block = true;
                test_block_depth = trimmed.chars().filter(|&c| c == '{').count() as i32
                    - trimmed.chars().filter(|&c| c == '}').count() as i32;
                continue;
            }
            // `#[cfg(test)]` on its own line: the next `mod X {` opens the block.
            if trimmed == "#[cfg(test)]" {
                pending_test_attr = true;
                continue;
            }
            if pending_test_attr {
                pending_test_attr = false;
                if trimmed.starts_with("mod ") && trimmed.ends_with('{') {
                    in_test_block = true;
                    test_block_depth = 1;
                    continue;
                }
                // Attribute was on a single item (`#[cfg(test)] fn foo …`) —
                // skip this line only, then resume normal counting.
                continue;
            }

            loc += 1;

            // Fix 2 — track re-export lines for the hub heuristic.
            if trimmed.starts_with("pub use ")
                || trimmed.starts_with("pub mod ")
                || (trimmed.starts_with("mod ") && trimmed.ends_with(';'))
            {
                reexport_lines += 1;
            }

            // use declarations
            if trimmed.starts_with("use ") {
                let use_path = trimmed
                    .strip_prefix("use ")
                    .unwrap_or("")
                    .trim_end_matches(';')
                    .trim();

                // Handle grouped uses: use std::{io, fs};
                if use_path.contains('{') {
                    let base = use_path
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .trim_end_matches("::");
                    let group = use_path
                        .split('{')
                        .nth(1)
                        .and_then(|s| s.split('}').next())
                        .unwrap_or("");

                    for item in group.split(',') {
                        let item = item.trim().split("::").next().unwrap_or("").trim();
                        if !item.is_empty() && item != "self" {
                            let full = format!("{}::{}", base, item);
                            imports.push(classify_rust_import(&full));
                        }
                    }
                    // Also add the base as import
                    if !base.is_empty() {
                        imports.push(classify_rust_import(base));
                    }
                } else {
                    // Simple use: use std::collections::HashMap;
                    // Strip the trailing item for module path
                    let path = use_path.split(" as ").next().unwrap_or(use_path).trim();
                    if !path.is_empty() {
                        imports.push(classify_rust_import(path));
                    }
                }
            }

            // mod declarations (internal module reference)
            if (trimmed.starts_with("pub mod ") || trimmed.starts_with("mod "))
                && trimmed.ends_with(';')
            {
                let mod_name = trimmed
                    .strip_prefix("pub mod ")
                    .or_else(|| trimmed.strip_prefix("mod "))
                    .unwrap_or("")
                    .trim_end_matches(';')
                    .trim();

                if !mod_name.is_empty() {
                    // mod foo; -> foo/mod.rs or foo.rs
                    let dir = file_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                    let candidates = if dir.is_empty() {
                        vec![format!("{}/mod.rs", mod_name), format!("{}.rs", mod_name)]
                    } else {
                        vec![
                            format!("{}/{}/mod.rs", dir, mod_name),
                            format!("{}/{}.rs", dir, mod_name),
                        ]
                    };
                    imports.push(RawImport {
                        module_path: candidates[0].clone(),
                        is_relative: true,
                    });
                }
            }

            // extern crate
            if trimmed.starts_with("extern crate ") {
                let crate_name = trimmed
                    .strip_prefix("extern crate ")
                    .unwrap_or("")
                    .trim_end_matches(';')
                    .split(" as ")
                    .next()
                    .unwrap_or("")
                    .trim();
                if !crate_name.is_empty() {
                    imports.push(RawImport {
                        module_path: crate_name.to_string(),
                        is_relative: false,
                    });
                }
            }

            // Count structs
            if (trimmed.starts_with("pub struct ") || trimmed.starts_with("struct "))
                && (trimmed.contains('{') || trimmed.contains('(') || trimmed.contains(';'))
            {
                class_count += 1;
            }

            // Count enums
            if (trimmed.starts_with("pub enum ") || trimmed.starts_with("enum "))
                && trimmed.contains('{')
            {
                class_count += 1;
            }

            // Count traits
            if (trimmed.starts_with("pub trait ") || trimmed.starts_with("trait "))
                && trimmed.contains('{')
            {
                class_count += 1;
            }

            // Count functions/methods
            if (trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("fn ")
                || trimmed.starts_with("async fn ")
                || trimmed.starts_with("pub(crate) fn ")
                || trimmed.starts_with("pub(crate) async fn ")
                || trimmed.starts_with("pub(super) fn "))
                && trimmed.contains('(')
            {
                function_count += 1;
            }
        }

        // Fix 2 — hub score: more than half the lines are re-exports.
        let is_hub = loc > 0 && (reexport_lines * 2 > loc);

        ParseResult {
            imports,
            class_count,
            function_count,
            loc,
            is_hub,
            has_framework_macros,
        }
    }
}

/// Classify a Rust use path as relative (crate/self/super) or external.
fn classify_rust_import(path: &str) -> RawImport {
    let is_relative =
        path.starts_with("crate::") || path.starts_with("self::") || path.starts_with("super::");

    // For crate-internal: convert crate::models::user to src/models/user.rs (approx)
    let module_path = if path.starts_with("crate::") {
        let rest = path.strip_prefix("crate::").unwrap_or(path);
        format!("src/{}", rest.replace("::", "/"))
    } else if path.starts_with("super::") {
        let rest = path.strip_prefix("super::").unwrap_or(path);
        format!("../{}", rest.replace("::", "/"))
    } else if path.starts_with("self::") {
        let rest = path.strip_prefix("self::").unwrap_or(path);
        rest.replace("::", "/")
    } else {
        // External: std::collections::HashMap -> std
        let root = path.split("::").next().unwrap_or(path);
        root.to_string()
    };

    RawImport {
        module_path,
        is_relative,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_use() {
        let parser = RustParser;
        let content = r#"
use std::collections::HashMap;
use std::io::{self, Read, Write};
use serde::{Deserialize, Serialize};

struct Config {
    name: String,
    port: u16,
}

fn main() {
    println!("hello");
}
"#;
        let result = parser.parse_file(content, "main.rs");
        assert!(result.imports.len() >= 3);
        assert_eq!(result.class_count, 1); // Config
        assert_eq!(result.function_count, 1); // main
    }

    #[test]
    fn test_crate_imports() {
        let parser = RustParser;
        let content = r#"
use crate::models::User;
use crate::services::auth;
use super::handler::Router;

pub struct Server {
    router: Router,
}

pub fn start() {}
pub async fn serve() {}
"#;
        let result = parser.parse_file(content, "src/server.rs");
        assert_eq!(result.imports.len(), 3);
        assert!(result.imports[0].is_relative); // crate::
        assert!(result.imports[1].is_relative); // crate::
        assert!(result.imports[2].is_relative); // super::
        assert_eq!(result.imports[0].module_path, "src/models/User");
        assert_eq!(result.class_count, 1);
        assert_eq!(result.function_count, 2);
    }

    #[test]
    fn test_mod_declarations() {
        let parser = RustParser;
        let content = r#"
mod handler;
pub mod routes;
mod models;

pub fn setup() {}
"#;
        let result = parser.parse_file(content, "src/lib.rs");
        assert_eq!(result.imports.len(), 3);
        assert!(result.imports[0].is_relative);
        assert_eq!(result.function_count, 1);
    }

    #[test]
    fn test_enums_traits() {
        let parser = RustParser;
        let content = r#"
pub enum Status {
    Active,
    Inactive,
}

pub trait Service {
    fn start(&self);
}

pub(crate) fn helper() {}
"#;
        let result = parser.parse_file(content, "lib.rs");
        assert_eq!(result.class_count, 2); // Status + Service
        assert_eq!(result.function_count, 2); // start (trait method) + helper
    }

    // ── Fix 1: test blocks excluded from LOC / function_count ──

    #[test]
    fn test_cfg_test_mod_block_excluded() {
        let parser = RustParser;
        let content = r#"
pub fn prod_fn() { 1 }
pub fn another_prod_fn() { 2 }

#[cfg(test)]
mod tests {
    use super::*;
    fn test_helper() {}

    #[test]
    fn test_one() { assert_eq!(prod_fn(), 1); }
    #[test]
    fn test_two() { assert_eq!(another_prod_fn(), 2); }
    #[test]
    fn test_three() { assert_eq!(prod_fn() + another_prod_fn(), 3); }
}
"#;
        let result = parser.parse_file(content, "lib.rs");
        // Only the two production functions should be counted.
        assert_eq!(
            result.function_count, 2,
            "tests inside #[cfg(test)] mod should be excluded"
        );
        // LOC should cover the two production functions, not the test bodies.
        assert!(
            result.loc <= 3,
            "LOC should not include the test module body, got {}",
            result.loc
        );
    }

    #[test]
    fn test_cfg_test_single_line_form() {
        let parser = RustParser;
        let content = r#"
fn prod() {}

#[cfg(test)] mod tests {
    fn inner() {}
    fn another() {}
}
"#;
        let result = parser.parse_file(content, "lib.rs");
        assert_eq!(result.function_count, 1);
    }

    // ── Fix 2: hub detection ──────────────────────────────────

    #[test]
    fn test_hub_module_detected() {
        let parser = RustParser;
        let content = r#"
//! Analyzer facade.

pub mod complexity;
pub mod coverage;
pub mod graph;
pub mod patterns;
pub mod docs;

pub use complexity::analyze_file;
pub use coverage::parse_coverage;
pub use graph::DependencyGraph;
"#;
        let result = parser.parse_file(content, "analyzer/mod.rs");
        assert!(
            result.is_hub,
            "file with only pub mod / pub use should be flagged is_hub"
        );
    }

    #[test]
    fn test_non_hub_module_not_flagged() {
        let parser = RustParser;
        let content = r#"
use crate::x;

pub fn foo() {}
pub fn bar() {}
pub fn baz() {}
pub fn qux() {}

pub use crate::y;
"#;
        let result = parser.parse_file(content, "lib.rs");
        assert!(
            !result.is_hub,
            "file with mostly real code should not be is_hub"
        );
    }

    // ── Fix 3: framework macro detection ──────────────────────

    #[test]
    fn test_framework_macro_tool_router_detected() {
        let parser = RustParser;
        let content = r#"
use rmcp::tool_router;

#[tool_router]
impl MyMcp {
    async fn a(&self) {}
    async fn b(&self) {}
}
"#;
        let result = parser.parse_file(content, "server.rs");
        assert!(
            result.has_framework_macros,
            "#[tool_router] should be detected"
        );
    }

    #[test]
    fn test_framework_macro_tool_handler_detected() {
        let parser = RustParser;
        let content = r#"
#[tool_handler]
impl ServerHandler for MyMcp {}
"#;
        let result = parser.parse_file(content, "server.rs");
        assert!(
            result.has_framework_macros,
            "#[tool_handler] should be detected"
        );
    }

    #[test]
    fn test_framework_macro_absent_is_false() {
        let parser = RustParser;
        let content = r#"
pub fn foo() {}
"#;
        let result = parser.parse_file(content, "lib.rs");
        assert!(!result.has_framework_macros);
    }
}
