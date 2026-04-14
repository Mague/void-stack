//! Python import parser.

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct PythonParser;

impl ImportParser for PythonParser {
    fn language(&self) -> Language {
        Language::Python
    }

    fn file_extensions(&self) -> &[&str] {
        &[".py"]
    }

    fn parse_file(&self, content: &str, _file_path: &str) -> ParseResult {
        let mut imports = Vec::new();
        let mut class_count = 0;
        let mut function_count = 0;
        let mut loc = 0;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            loc += 1;

            // import X, import X.Y
            if trimmed.starts_with("import ") && !trimmed.starts_with("import (") {
                let rest = trimmed.strip_prefix("import ").unwrap().trim();
                // Handle "import X, Y, Z"
                for part in rest.split(',') {
                    let module = part.split(" as ").next().unwrap_or("").trim();
                    if !module.is_empty() {
                        imports.push(RawImport {
                            module_path: module.to_string(),
                            is_relative: false,
                        });
                    }
                }
            }

            // from X import Y, from .X import Y, from ..X import Y
            if trimmed.starts_with("from ") && trimmed.contains(" import ") {
                let after_from = trimmed.strip_prefix("from ").unwrap();
                let module = after_from.split(" import ").next().unwrap_or("").trim();
                if !module.is_empty() {
                    let is_relative = module.starts_with('.');
                    let clean_module = module.trim_start_matches('.');
                    imports.push(RawImport {
                        module_path: if is_relative && clean_module.is_empty() {
                            ".".to_string()
                        } else if is_relative {
                            format!("./{}", clean_module.replace('.', "/"))
                        } else {
                            module.to_string()
                        },
                        is_relative,
                    });
                }
            }

            // Count classes
            if trimmed.starts_with("class ") && trimmed.contains(':') {
                class_count += 1;
            }

            // Count functions (top-level and methods)
            if (trimmed.starts_with("def ") || trimmed.starts_with("async def "))
                && trimmed.contains('(')
            {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_imports() {
        let parser = PythonParser;
        let content = r#"
import os
import sys
from pathlib import Path
from .utils import helper
from ..models.user import User
from fastapi import FastAPI

class MyService:
    pass

def main():
    pass

async def handler():
    pass
"#;
        let result = parser.parse_file(content, "app.py");
        assert_eq!(result.imports.len(), 6);
        assert_eq!(result.class_count, 1);
        assert_eq!(result.function_count, 2);

        // Check relative import detection
        let relative_count = result.imports.iter().filter(|i| i.is_relative).count();
        assert_eq!(relative_count, 2);
    }

    #[test]
    fn test_multi_import() {
        let parser = PythonParser;
        let content = "import os, sys, json\n";
        let result = parser.parse_file(content, "test.py");
        assert_eq!(result.imports.len(), 3);
    }

    #[test]
    fn test_aliased_import() {
        let parser = PythonParser;
        let content = "import numpy as np\nfrom datetime import datetime as dt\n";
        let result = parser.parse_file(content, "test.py");
        assert_eq!(result.imports.len(), 2);
        assert_eq!(result.imports[0].module_path, "numpy");
    }
}
