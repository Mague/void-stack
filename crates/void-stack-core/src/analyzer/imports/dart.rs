//! Dart import parser.
//!
//! Parses Dart imports, exports, and part directives:
//! ```text
//! import 'package:flutter/material.dart';
//! import 'dart:async';
//! import '../models/user.dart';
//! export 'src/widget.dart';
//! part 'model.g.dart';
//! ```

use super::{ImportParser, ParseResult, RawImport};
use crate::analyzer::graph::Language;

pub struct DartParser;

impl ImportParser for DartParser {
    fn language(&self) -> Language {
        Language::Dart
    }

    fn file_extensions(&self) -> &[&str] {
        &[".dart"]
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

            // import 'package:name/path.dart';
            // import 'dart:async';
            // import '../relative/path.dart';
            // import 'relative.dart';
            if trimmed.starts_with("import ")
                && let Some(path) = extract_dart_string(trimmed)
            {
                let is_relative = path.starts_with('.') || path.starts_with('/');
                let is_dart_core = path.starts_with("dart:");

                if !is_dart_core {
                    imports.push(RawImport {
                        module_path: normalize_dart_import(&path),
                        is_relative,
                    });
                }
            }

            // export 'src/widget.dart';
            if trimmed.starts_with("export ")
                && !trimmed.contains("class ")
                && let Some(path) = extract_dart_string(trimmed)
            {
                let is_relative = path.starts_with('.') || !path.starts_with("package:");
                let is_dart_core = path.starts_with("dart:");

                if !is_dart_core {
                    imports.push(RawImport {
                        module_path: normalize_dart_import(&path),
                        is_relative,
                    });
                }
            }

            // part 'model.g.dart';  (generated code references)
            if (trimmed.starts_with("part '") || trimmed.starts_with("part \""))
                && let Some(path) = extract_dart_string(trimmed)
            {
                imports.push(RawImport {
                    module_path: path,
                    is_relative: true,
                });
            }

            // Count classes: class X { / abstract class X { / mixin X {
            if (trimmed.starts_with("class ")
                || trimmed.starts_with("abstract class ")
                || trimmed.starts_with("mixin "))
                && (trimmed.contains('{')
                    || trimmed.contains("extends")
                    || trimmed.contains("implements"))
            {
                class_count += 1;
            }

            // Enums
            if trimmed.starts_with("enum ") && trimmed.contains('{') {
                class_count += 1;
            }

            // Count top-level functions and methods
            // Dart functions: ReturnType name( or void name( or Future<X> name(
            // Also: static/async modifiers
            let func_line = trimmed
                .strip_prefix("static ")
                .or_else(|| trimmed.strip_prefix("@override "))
                .unwrap_or(trimmed);
            let func_line = func_line.strip_prefix("async ").unwrap_or(func_line);

            if !func_line.starts_with("if ")
                && !func_line.starts_with("for ")
                && !func_line.starts_with("while ")
                && !func_line.starts_with("return ")
                && !func_line.starts_with("//")
                && !func_line.starts_with("class ")
                && !func_line.starts_with("abstract ")
                && !func_line.starts_with("import ")
                && !func_line.starts_with("export ")
                && !func_line.starts_with("var ")
                && !func_line.starts_with("final ")
                && !func_line.starts_with("const ")
                && !func_line.starts_with("late ")
            {
                // Pattern: Type name( or name(
                if let Some(paren) = func_line.find('(') {
                    let before = &func_line[..paren];
                    let parts: Vec<&str> = before.split_whitespace().collect();
                    // Must have at least "name" before (, and name must be lowercase start (convention)
                    if !parts.is_empty() && parts.len() <= 3 {
                        let name = parts.last().unwrap_or(&"");
                        if !name.is_empty()
                            && name
                                .chars()
                                .next()
                                .map(|c| c.is_lowercase() || c == '_')
                                .unwrap_or(false)
                            && (func_line.contains('{')
                                || func_line.contains("=>")
                                || func_line.ends_with(')')
                                || func_line.contains(") {")
                                || func_line.contains(") async"))
                            && !matches!(
                                *name,
                                "if" | "for" | "while" | "switch" | "catch" | "import" | "export"
                            )
                        {
                            function_count += 1;
                        }
                    }
                    // Constructor: ClassName( or ClassName.named(
                    if parts.len() == 1 {
                        let name = parts[0];
                        if name
                            .chars()
                            .next()
                            .map(|c| c.is_uppercase())
                            .unwrap_or(false)
                            && (func_line.contains('{') || func_line.contains(';'))
                        {
                            function_count += 1;
                        }
                    }
                }
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

/// Extract string from import/export statement.
fn extract_dart_string(line: &str) -> Option<String> {
    // Find first ' or "
    let single = line.find('\'');
    let double = line.find('"');

    let (quote_char, start) = match (single, double) {
        (Some(s), Some(d)) => {
            if s < d {
                ('\'', s)
            } else {
                ('"', d)
            }
        }
        (Some(s), None) => ('\'', s),
        (None, Some(d)) => ('"', d),
        (None, None) => return None,
    };

    let rest = &line[start + 1..];
    let end = rest.find(quote_char)?;
    Some(rest[..end].to_string())
}

/// Normalize a Dart import path.
/// "package:my_app/src/models/user.dart" -> "lib/src/models/user.dart"
fn normalize_dart_import(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("package:") {
        // package:name/path.dart -> skip package name, keep path
        if let Some(slash) = rest.find('/') {
            return format!("lib{}", &rest[slash..]);
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dart_imports() {
        let parser = DartParser;
        let content = r#"
import 'dart:async';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:my_app/src/models/user.dart';
import '../services/auth_service.dart';

class UserWidget extends StatefulWidget {
  @override
  State<UserWidget> createState() => _UserWidgetState();
}

class _UserWidgetState extends State<UserWidget> {
  void initState() {
    super.initState();
  }

  Future<void> fetchData() async {
    // ...
  }

  Widget build(BuildContext context) {
    return Container();
  }
}
"#;
        let result = parser.parse_file(content, "user_widget.dart");
        // dart:async and dart:io are skipped (core)
        assert_eq!(result.imports.len(), 3);
        assert_eq!(result.imports[0].module_path, "lib/material.dart"); // flutter
        assert_eq!(result.imports[1].module_path, "lib/src/models/user.dart");
        assert!(result.imports[2].is_relative);
        assert_eq!(result.class_count, 2); // UserWidget + _UserWidgetState
        assert!(result.function_count >= 3); // createState, initState, fetchData, build
    }

    #[test]
    fn test_dart_exports_and_parts() {
        let parser = DartParser;
        let content = r#"
export 'src/models/user.dart';
export 'src/services/auth.dart';
part 'user.g.dart';
part 'user.freezed.dart';

class User {
  final String name;
  User(this.name);
}

enum UserRole {
  admin,
  user,
}
"#;
        let result = parser.parse_file(content, "user.dart");
        assert_eq!(result.imports.len(), 4); // 2 exports + 2 parts
        assert_eq!(result.class_count, 2); // User + UserRole
    }

    #[test]
    fn test_dart_package_normalize() {
        assert_eq!(
            normalize_dart_import("package:my_app/src/widget.dart"),
            "lib/src/widget.dart"
        );
        assert_eq!(
            normalize_dart_import("../models/user.dart"),
            "../models/user.dart"
        );
    }
}
