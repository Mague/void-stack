//! Detect unsafe error handling patterns across languages.
//!
//! - Rust: `.unwrap()` / `.expect()` outside tests and main.rs
//! - Python: bare `except:`, `except Exception: pass`, `except BaseException`
//! - JavaScript/TypeScript: empty catch blocks
//! - Go: error discarded with `_ = err` or `_ :=`
//! - Dart: bare `catch` without `on` clause

use super::super::findings::{FindingCategory, SecurityFinding, Severity};
use super::{FileInfo, adjust_severity};

pub fn scan_unsafe_error_handling(files: &[FileInfo], findings: &mut Vec<SecurityFinding>) {
    for file in files {
        match file.ext.as_str() {
            "rs" => scan_rust_unwrap(file, findings),
            "py" => scan_python_bare_except(file, findings),
            "js" | "jsx" | "ts" | "tsx" => scan_js_empty_catch(file, findings),
            "go" => scan_go_error_discard(file, findings),
            "dart" => scan_dart_bare_catch(file, findings),
            _ => {}
        }
    }
}

/// Rust: .unwrap() and .expect() outside tests and main.rs
fn scan_rust_unwrap(file: &FileInfo, findings: &mut Vec<SecurityFinding>) {
    // Skip test files and main.rs/build.rs
    let lower = file.rel_path.to_lowercase();
    if file.is_test_file || lower.ends_with("main.rs") || lower.ends_with("build.rs") {
        return;
    }

    for (i, line) in file.content.lines().enumerate() {
        let trimmed = line.trim();
        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            continue;
        }
        // Skip lines that are part of test modules
        if trimmed.contains("#[cfg(test)]") || trimmed.contains("#[test]") {
            return; // Stop scanning — rest of file is likely test code
        }

        let has_unwrap = trimmed.contains(".unwrap()");
        let has_expect = trimmed.contains(".expect(");

        if has_unwrap || has_expect {
            let method = if has_unwrap { ".unwrap()" } else { ".expect()" };
            findings.push(SecurityFinding {
                id: format!("ERR-RUST-{}", i + 1),
                severity: adjust_severity(Severity::Medium, file.is_test_file),
                category: FindingCategory::UnsafeErrorHandling,
                title: format!("Uso de {} en codigo de produccion", method),
                description: format!(
                    "'{}' puede causar panic! en runtime. Usar '?' o match para manejar errores correctamente.",
                    method
                ),
                file_path: Some(file.rel_path.clone()),
                line_number: Some((i + 1) as u32),
                remediation: format!(
                    "Reemplazar {} con '?' para propagar errores, o usar 'match'/'if let' para manejarlos explicitamente.",
                    method
                ),
            });
        }
    }
}

/// Python: bare except, except Exception with pass, except BaseException
fn scan_python_bare_except(file: &FileInfo, findings: &mut Vec<SecurityFinding>) {
    let lines: Vec<&str> = file.content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // bare except:
        if trimmed == "except:" || trimmed.starts_with("except: ") {
            findings.push(SecurityFinding {
                id: format!("ERR-PY-BARE-{}", i + 1),
                severity: adjust_severity(Severity::High, file.is_test_file),
                category: FindingCategory::UnsafeErrorHandling,
                title: "Bare except sin tipo especifico".into(),
                description: "Captura todas las excepciones incluyendo KeyboardInterrupt y SystemExit, ocultando bugs criticos.".into(),
                file_path: Some(file.rel_path.clone()),
                line_number: Some((i + 1) as u32),
                remediation: "Especificar el tipo de excepcion: 'except ValueError:' o al menos 'except Exception:' con logging.".into(),
            });
            continue;
        }

        // except Exception: pass / except Exception as e: pass
        if trimmed.starts_with("except Exception:") || trimmed.starts_with("except Exception as ") {
            // Check if next non-empty line is just 'pass'
            let next = lines.get(i + 1).map(|l| l.trim());
            if next == Some("pass") {
                findings.push(SecurityFinding {
                    id: format!("ERR-PY-PASS-{}", i + 1),
                    severity: adjust_severity(Severity::Medium, file.is_test_file),
                    category: FindingCategory::UnsafeErrorHandling,
                    title: "except Exception con pass (error silenciado)".into(),
                    description: "Captura y descarta silenciosamente todas las excepciones, ocultando errores.".into(),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Agregar logging: 'except Exception as e: logger.exception(e)' o manejar el error.".into(),
                });
                continue;
            }
        }

        // except BaseException
        if trimmed.starts_with("except BaseException") {
            findings.push(SecurityFinding {
                id: format!("ERR-PY-BASE-{}", i + 1),
                severity: adjust_severity(Severity::High, file.is_test_file),
                category: FindingCategory::UnsafeErrorHandling,
                title: "except BaseException captura senales del sistema".into(),
                description: "Captura KeyboardInterrupt y SystemExit, impidiendo la terminacion limpia del proceso.".into(),
                file_path: Some(file.rel_path.clone()),
                line_number: Some((i + 1) as u32),
                remediation: "Usar 'except Exception:' en vez de 'except BaseException:'.".into(),
            });
        }
    }
}

/// JavaScript/TypeScript: empty catch blocks
fn scan_js_empty_catch(file: &FileInfo, findings: &mut Vec<SecurityFinding>) {
    let lines: Vec<&str> = file.content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Pattern: catch(e) {} or catch(_) {} or catch { } on same line
        if trimmed.contains("catch") && trimmed.contains('{') && trimmed.contains('}') {
            let after_catch = trimmed.split("catch").last().unwrap_or("");
            // Check if the block between { and } is empty or whitespace
            if let Some(open) = after_catch.find('{')
                && let Some(close) = after_catch[open..].find('}')
            {
                let body = after_catch[open + 1..open + close].trim();
                if body.is_empty() {
                    findings.push(SecurityFinding {
                            id: format!("ERR-JS-EMPTY-{}", i + 1),
                            severity: adjust_severity(Severity::Medium, file.is_test_file),
                            category: FindingCategory::UnsafeErrorHandling,
                            title: "Bloque catch vacio".into(),
                            description: "Los errores capturados se descartan silenciosamente, ocultando problemas.".into(),
                            file_path: Some(file.rel_path.clone()),
                            line_number: Some((i + 1) as u32),
                            remediation: "Agregar logging: 'catch(e) { console.error(e); }' o re-lanzar el error.".into(),
                        });
                    continue;
                }
            }
        }

        // Multi-line: catch(...) {\n}
        if (trimmed.starts_with("catch")
            || (trimmed.starts_with("} catch") && trimmed.ends_with('{')))
            && let Some(next) = lines.get(i + 1)
        {
            let next_trimmed = next.trim();
            if next_trimmed == "}"
                || next_trimmed == "} finally"
                || next_trimmed.starts_with("} finally")
            {
                findings.push(SecurityFinding {
                    id: format!("ERR-JS-EMPTY-{}", i + 1),
                    severity: adjust_severity(Severity::Medium, file.is_test_file),
                    category: FindingCategory::UnsafeErrorHandling,
                    title: "Bloque catch vacio".into(),
                    description: "Los errores capturados se descartan silenciosamente.".into(),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Agregar logging o manejar el error en el bloque catch.".into(),
                });
            }
        }
    }
}

/// Go: error assigned to _ (discarded)
fn scan_go_error_discard(file: &FileInfo, findings: &mut Vec<SecurityFinding>) {
    for (i, line) in file.content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }

        // Pattern: _ = someFunc() or _, err := (where _ discards the error)
        // Look for patterns where error return is explicitly discarded
        if (trimmed.contains("_ =") || trimmed.contains("_ :=")) && !trimmed.starts_with("//") {
            // Heuristic: if the line has a function call and _ is used
            if trimmed.contains('(') && trimmed.contains(')') {
                // Check it's likely discarding an error (common Go pattern)
                let has_err_context = trimmed.contains("err")
                    || trimmed.ends_with(')')
                    || trimmed.contains(", _")
                    || trimmed.starts_with("_ =")
                    || trimmed.starts_with("_ :=");

                if has_err_context {
                    findings.push(SecurityFinding {
                        id: format!("ERR-GO-DISCARD-{}", i + 1),
                        severity: adjust_severity(Severity::Medium, file.is_test_file),
                        category: FindingCategory::UnsafeErrorHandling,
                        title: "Error descartado con _".into(),
                        description: "El error de retorno se asigna a '_', descartandolo sin verificar.".into(),
                        file_path: Some(file.rel_path.clone()),
                        line_number: Some((i + 1) as u32),
                        remediation: "Manejar el error: 'if err != nil { return err }' o loggear con 'log.Printf'.".into(),
                    });
                }
            }
        }
    }
}

/// Dart: catch without on clause
fn scan_dart_bare_catch(file: &FileInfo, findings: &mut Vec<SecurityFinding>) {
    for (i, line) in file.content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }

        // Pattern: "catch" without a preceding "on SomeType" on the same line
        // Valid: `on FormatException catch (e)` — has `on` before `catch`
        // Invalid: `} catch (e) {` or `catch (e) {` without `on`
        if trimmed.contains("catch") && trimmed.contains('(') {
            let before_catch = trimmed.split("catch").next().unwrap_or("");
            if !before_catch.contains(" on ") && !before_catch.trim().starts_with("on ") {
                findings.push(SecurityFinding {
                    id: format!("ERR-DART-BARE-{}", i + 1),
                    severity: adjust_severity(Severity::Medium, file.is_test_file),
                    category: FindingCategory::UnsafeErrorHandling,
                    title: "catch sin clausula on especifica".into(),
                    description: "Captura todos los tipos de excepcion sin filtrar, ocultando errores inesperados.".into(),
                    file_path: Some(file.rel_path.clone()),
                    line_number: Some((i + 1) as u32),
                    remediation: "Especificar el tipo: 'on FormatException catch (e)' o al menos loggear el error.".into(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file(path: &str, ext: &str, content: &str) -> FileInfo {
        FileInfo {
            rel_path: path.into(),
            content: content.into(),
            ext: ext.into(),
            is_test_file: false,
        }
    }

    // ── Rust ───────────────────────────────────────────────────

    #[test]
    fn test_rust_unwrap() {
        let file = make_file(
            "src/service.rs",
            "rs",
            "let val = result.unwrap();\nlet x = opt.expect(\"fail\");",
        );
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert_eq!(findings.len(), 2);
        assert!(findings[0].title.contains(".unwrap()"));
        assert!(findings[1].title.contains(".expect()"));
    }

    #[test]
    fn test_rust_unwrap_skipped_in_tests() {
        let file = FileInfo {
            rel_path: "tests/test_api.rs".into(),
            content: "let val = result.unwrap();".into(),
            ext: "rs".into(),
            is_test_file: true,
        };
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_rust_unwrap_skipped_in_main() {
        let file = make_file("src/main.rs", "rs", "let val = config.unwrap();");
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    // ── Python ─────────────────────────────────────────────────

    #[test]
    fn test_python_bare_except() {
        let file = make_file("app.py", "py", "try:\n    do_stuff()\nexcept:\n    pass");
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("Bare except")));
    }

    #[test]
    fn test_python_except_exception_pass() {
        let file = make_file(
            "app.py",
            "py",
            "try:\n    do_stuff()\nexcept Exception as e:\n    pass",
        );
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("pass")));
    }

    #[test]
    fn test_python_except_base_exception() {
        let file = make_file(
            "app.py",
            "py",
            "try:\n    do_stuff()\nexcept BaseException:\n    log(e)",
        );
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("BaseException")));
    }

    // ── JavaScript/TypeScript ──────────────────────────────────

    #[test]
    fn test_js_empty_catch_single_line() {
        let file = make_file("handler.js", "js", "try { doStuff() } catch(e) {}");
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("catch vacio")));
    }

    #[test]
    fn test_js_empty_catch_multiline() {
        let file = make_file("handler.ts", "ts", "try {\n  doStuff()\n} catch(e) {\n}");
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("catch vacio")));
    }

    #[test]
    fn test_js_catch_with_body_ok() {
        let file = make_file(
            "handler.js",
            "js",
            "try { x() } catch(e) { console.error(e) }",
        );
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.is_empty());
    }

    // ── Go ─────────────────────────────────────────────────────

    #[test]
    fn test_go_error_discard() {
        let file = make_file("main.go", "go", "_ = json.Unmarshal(data, &obj)");
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("descartado")));
    }

    // ── Dart ───────────────────────────────────────────────────

    #[test]
    fn test_dart_bare_catch() {
        let file = make_file(
            "service.dart",
            "dart",
            "try {\n  doStuff();\n} catch (e) {\n  print(e);\n}",
        );
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(
            findings
                .iter()
                .any(|f| f.title.contains("catch sin clausula on"))
        );
    }

    #[test]
    fn test_dart_on_clause_ok() {
        let file = make_file(
            "service.dart",
            "dart",
            "try {\n  doStuff();\n} on FormatException catch (e) {\n  print(e);\n}",
        );
        let mut findings = Vec::new();
        scan_unsafe_error_handling(&[file], &mut findings);
        assert!(findings.is_empty());
    }
}
