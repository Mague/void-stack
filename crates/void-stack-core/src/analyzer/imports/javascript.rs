//! JavaScript/TypeScript import parser.

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct JsParser;

impl ImportParser for JsParser {
    fn language(&self) -> Language {
        Language::JavaScript
    }

    fn file_extensions(&self) -> &[&str] {
        &[".js", ".ts", ".jsx", ".tsx", ".mjs"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
        let mut imports = Vec::new();
        let mut class_count = 0;
        let mut function_count = 0;
        let mut loc = 0;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            loc += 1;

            // import ... from "module"  /  import "module"
            if trimmed.starts_with("import ")
                && let Some(module) = extract_module_from_import(trimmed)
            {
                imports.push(RawImport {
                    module_path: module.clone(),
                    is_relative: module.starts_with('.'),
                });
            }

            // const X = require("module")  /  require("module")
            if trimmed.contains("require(")
                && let Some(module) = extract_require(trimmed)
            {
                imports.push(RawImport {
                    module_path: module.clone(),
                    is_relative: module.starts_with('.'),
                });
            }

            // export ... from "module" (re-exports)
            if trimmed.starts_with("export ")
                && trimmed.contains(" from ")
                && let Some(module) = extract_from_string(trimmed)
            {
                imports.push(RawImport {
                    module_path: module.clone(),
                    is_relative: module.starts_with('.'),
                });
            }

            // Count classes
            if (trimmed.starts_with("class ") || trimmed.contains(" class "))
                && (trimmed.contains('{') || trimmed.contains("extends"))
            {
                class_count += 1;
            }

            // Count functions
            if trimmed.starts_with("function ")
                || trimmed.starts_with("async function ")
                || trimmed.starts_with("export function ")
                || trimmed.starts_with("export async function ")
                || trimmed.starts_with("export default function")
            {
                function_count += 1;
            }

            // Arrow functions assigned to const/let/var
            if (trimmed.starts_with("const ")
                || trimmed.starts_with("let ")
                || trimmed.starts_with("export const "))
                && (trimmed.contains("=>") || trimmed.contains("= function"))
            {
                function_count += 1;
            }
        }

        ParseResult {
            imports,
            class_count,
            function_count,
            loc,
        }
    }
}

/// Extract module path from: import ... from "module" or import "module"
fn extract_module_from_import(line: &str) -> Option<String> {
    // import X from "module"
    if let Some(module) = extract_from_string(line) {
        return Some(module);
    }
    // import "module" (side-effect import)
    let after_import = line.strip_prefix("import ")?.trim();
    extract_string(after_import)
}

/// Extract the string after `from` keyword.
fn extract_from_string(line: &str) -> Option<String> {
    let from_pos = line.rfind(" from ")?;
    let after = line[from_pos + 6..].trim();
    extract_string(after)
}

/// Extract a string literal (single, double, or backtick quoted).
fn extract_string(s: &str) -> Option<String> {
    let trimmed = s.trim().trim_end_matches(';');
    let quote = trimmed.chars().next()?;
    if !matches!(quote, '"' | '\'' | '`') {
        return None;
    }
    let rest = &trimmed[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Extract module from require("module").
fn extract_require(line: &str) -> Option<String> {
    let pos = line.find("require(")?;
    let after = &line[pos + 8..];
    extract_string(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_es_imports() {
        let parser = JsParser;
        let content = r#"
import React from "react"
import { useState } from 'react'
import "./styles.css"
import UserService from './services/user'
import * as utils from "../utils"

const handler = async () => {}

export function main() {}

class App extends Component {
}
"#;
        let result = parser.parse_file(content, "app.tsx");
        assert_eq!(result.imports.len(), 5);
        assert_eq!(result.class_count, 1);
        assert_eq!(result.function_count, 2); // handler + main

        let external = result.imports.iter().filter(|i| !i.is_relative).count();
        assert_eq!(external, 2); // react x2
        let relative = result.imports.iter().filter(|i| i.is_relative).count();
        assert_eq!(relative, 3);
    }

    #[test]
    fn test_require() {
        let parser = JsParser;
        let content = r#"
const express = require("express")
const db = require("./db")
const { Router } = require('express')
"#;
        let result = parser.parse_file(content, "app.js");
        assert_eq!(result.imports.len(), 3);
    }

    #[test]
    fn test_reexport() {
        let parser = JsParser;
        let content = r#"
export { default } from './components/Button'
export * from "../utils"
"#;
        let result = parser.parse_file(content, "index.ts");
        assert_eq!(result.imports.len(), 2);
    }
}
