//! Coverage hint generation per detected language.

use std::collections::HashSet;

use crate::analyzer::graph::{DependencyGraph, Language};

/// Generate a coverage hint based on detected languages.
pub(crate) fn coverage_hint(graph: &DependencyGraph) -> Option<String> {
    let languages: HashSet<_> = graph.modules.iter().map(|m| m.language).collect();
    if languages.is_empty() {
        return None;
    }

    let mut hints = Vec::new();
    hints.push("Para generar reportes de cobertura, ejecutar:".to_string());

    if languages.contains(&Language::Rust) {
        hints.push("- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)".into());
    }
    if languages.contains(&Language::Python) {
        hints.push("- **Python**: `pip install pytest-cov && pytest --cov --cov-report=xml` (genera `coverage.xml`)".into());
    }
    if languages.contains(&Language::JavaScript) || languages.contains(&Language::TypeScript) {
        hints.push(
            "- **JS/TS**: `npx c8 --reporter=lcov npm test` (genera `coverage/lcov.info`)".into(),
        );
    }
    if languages.contains(&Language::Go) {
        hints.push(
            "- **Go**: `go test -coverprofile=coverage.out ./...` (genera `coverage.out`)".into(),
        );
    }
    if languages.contains(&Language::Dart) {
        hints.push("- **Flutter**: `flutter test --coverage` (genera `coverage/lcov.info`)".into());
    }

    if hints.len() == 1 {
        return None; // No language-specific hints
    }

    Some(hints.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::graph::{ArchLayer, ModuleNode};

    fn make_graph_with_lang(lang: Language) -> DependencyGraph {
        DependencyGraph {
            root_path: "/project".into(),
            primary_language: lang,
            modules: vec![ModuleNode {
                path: "main".into(),
                language: lang,
                layer: ArchLayer::Service,
                loc: 100,
                class_count: 0,
                function_count: 5,
            }],
            edges: vec![],
            external_deps: std::collections::HashSet::new(),
        }
    }

    #[test]
    fn test_coverage_hint_python() {
        let hint = coverage_hint(&make_graph_with_lang(Language::Python));
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("Python"));
    }

    #[test]
    fn test_coverage_hint_rust() {
        let hint = coverage_hint(&make_graph_with_lang(Language::Rust));
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("Rust"));
    }

    #[test]
    fn test_coverage_hint_js_ts() {
        let hint = coverage_hint(&make_graph_with_lang(Language::JavaScript));
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("JS/TS"));
    }

    #[test]
    fn test_coverage_hint_go() {
        let hint = coverage_hint(&make_graph_with_lang(Language::Go));
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("Go"));
    }

    #[test]
    fn test_coverage_hint_dart() {
        let hint = coverage_hint(&make_graph_with_lang(Language::Dart));
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("Flutter"));
    }

    #[test]
    fn test_coverage_hint_empty() {
        let graph = DependencyGraph {
            root_path: "/project".into(),
            primary_language: Language::Python,
            modules: vec![],
            edges: vec![],
            external_deps: std::collections::HashSet::new(),
        };
        assert!(coverage_hint(&graph).is_none());
    }
}
