//! Anti-pattern detection.

use super::super::graph::*;

/// Severity level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Low => write!(f, "Low"),
            Severity::Medium => write!(f, "Medium"),
            Severity::High => write!(f, "High"),
        }
    }
}

/// Kind of anti-pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AntiPatternKind {
    GodClass,
    CircularDependency,
    FatController,
    NoServiceLayer,
    ExcessiveCoupling,
}

impl std::fmt::Display for AntiPatternKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AntiPatternKind::GodClass => write!(f, "God Class"),
            AntiPatternKind::CircularDependency => write!(f, "Circular Dependency"),
            AntiPatternKind::FatController => write!(f, "Fat Controller"),
            AntiPatternKind::NoServiceLayer => write!(f, "No Service Layer"),
            AntiPatternKind::ExcessiveCoupling => write!(f, "Excessive Coupling"),
        }
    }
}

/// A detected anti-pattern.
#[derive(Debug, Clone)]
pub struct AntiPattern {
    pub kind: AntiPatternKind,
    pub description: String,
    pub affected_modules: Vec<String>,
    pub severity: Severity,
    pub suggestion: String,
}

/// Run all anti-pattern detections.
pub fn detect_antipatterns(graph: &DependencyGraph) -> Vec<AntiPattern> {
    let mut results = Vec::new();

    detect_god_classes(graph, &mut results);
    detect_circular_deps(graph, &mut results);
    detect_fat_controllers(graph, &mut results);
    detect_no_service_layer(graph, &mut results);
    detect_excessive_coupling(graph, &mut results);

    results
}

/// God class LOC/function thresholds by language.
/// Returns (loc_medium, loc_high, fn_medium, fn_high).
fn god_class_thresholds(language: Language) -> (usize, usize, usize, usize) {
    match language {
        Language::Dart => (800, 1500, 25, 40), // Flutter has more boilerplate
        Language::Rust => (600, 1200, 20, 35), // Rust is more verbose than Python
        Language::Go => (500, 1000, 15, 25),
        _ => (500, 1000, 15, 25), // Python, JS/TS — default
    }
}

/// Files with too many functions or too many LOC.
///
/// Three false-positive suppressions apply:
/// 1. Test modules (`ArchLayer::Test`) are skipped outright.
/// 2. Hub files (>50% `pub use`/`pub mod` lines) are skipped — those are
///    facade/re-export modules, not god classes.
/// 3. Files using framework routing macros (rmcp `#[tool_router]` etc.)
///    ignore the `function_count` trigger — one-function-per-tool is
///    inherent to the pattern. They can still trip on LOC.
fn detect_god_classes(graph: &DependencyGraph, out: &mut Vec<AntiPattern>) {
    for m in &graph.modules {
        if m.layer == ArchLayer::Test {
            continue;
        }
        // Fix 2: re-export hubs never qualify as god classes.
        if m.is_hub {
            continue;
        }

        let (loc_med, loc_high, fn_med, fn_high) = god_class_thresholds(m.language);

        // Fix 3: framework-macro files drop the function_count trigger.
        let fn_over_med = !m.has_framework_macros && m.function_count > fn_med;
        let fn_over_high = !m.has_framework_macros && m.function_count > fn_high;

        let is_god = m.loc > loc_med || fn_over_med;
        if is_god {
            let reason = if m.loc > loc_med && fn_over_med {
                format!("{} LOC y {} funciones", m.loc, m.function_count)
            } else if m.loc > loc_med {
                format!("{} LOC", m.loc)
            } else {
                format!("{} funciones", m.function_count)
            };
            out.push(AntiPattern {
                kind: AntiPatternKind::GodClass,
                description: format!("'{}' es demasiado grande ({})", m.path, reason),
                affected_modules: vec![m.path.clone()],
                severity: if m.loc > loc_high || fn_over_high {
                    Severity::High
                } else {
                    Severity::Medium
                },
                suggestion: format!(
                    "Dividir '{}' en modulos mas pequenos con responsabilidades claras",
                    m.path
                ),
            });
        }
    }
}

/// Circular dependencies between modules.
fn detect_circular_deps(graph: &DependencyGraph, out: &mut Vec<AntiPattern>) {
    let cycles = graph.find_cycles();
    for cycle in &cycles {
        let chain = cycle.join(" <-> ");
        out.push(AntiPattern {
            kind: AntiPatternKind::CircularDependency,
            description: format!("Dependencia circular: {}", chain),
            affected_modules: cycle.clone(),
            severity: Severity::High,
            suggestion: "Extraer la interfaz comun a un modulo separado o invertir la dependencia"
                .to_string(),
        });
    }
}

/// Controllers with too much logic (high LOC or importing repositories directly).
fn detect_fat_controllers(graph: &DependencyGraph, out: &mut Vec<AntiPattern>) {
    let module_layers: std::collections::HashMap<&str, ArchLayer> = graph
        .modules
        .iter()
        .map(|m| (m.path.as_str(), m.layer))
        .collect();

    for m in &graph.modules {
        if m.layer != ArchLayer::Controller {
            continue;
        }

        // Check if controller is too large
        if m.loc > 200 {
            out.push(AntiPattern {
                kind: AntiPatternKind::FatController,
                description: format!(
                    "Controller '{}' tiene {} LOC — demasiada logica",
                    m.path, m.loc
                ),
                affected_modules: vec![m.path.clone()],
                severity: if m.loc > 400 {
                    Severity::High
                } else {
                    Severity::Medium
                },
                suggestion: "Mover la logica de negocio a una capa de servicio".to_string(),
            });
        }

        // Check if controller imports repository directly
        for edge in &graph.edges {
            if edge.from == m.path
                && !edge.is_external
                && let Some(ArchLayer::Repository) = module_layers.get(edge.to.as_str())
            {
                out.push(AntiPattern {
                        kind: AntiPatternKind::FatController,
                        description: format!("Controller '{}' importa directamente el repositorio '{}' — falta capa de servicio", m.path, edge.to),
                        affected_modules: vec![m.path.clone(), edge.to.clone()],
                        severity: Severity::Medium,
                        suggestion: "Crear un servicio intermedio entre el controller y el repositorio".to_string(),
                    });
            }
        }
    }
}

/// Controllers without a service layer.
fn detect_no_service_layer(graph: &DependencyGraph, out: &mut Vec<AntiPattern>) {
    let has_controllers = graph
        .modules
        .iter()
        .any(|m| m.layer == ArchLayer::Controller);
    let has_services = graph.modules.iter().any(|m| m.layer == ArchLayer::Service);

    if has_controllers && !has_services && graph.modules.len() > 5 {
        let controllers: Vec<String> = graph
            .modules
            .iter()
            .filter(|m| m.layer == ArchLayer::Controller)
            .map(|m| m.path.clone())
            .collect();

        out.push(AntiPattern {
            kind: AntiPatternKind::NoServiceLayer,
            description: format!(
                "Proyecto tiene {} controllers pero ninguna capa de servicio",
                controllers.len()
            ),
            affected_modules: controllers,
            severity: Severity::Medium,
            suggestion:
                "Crear una capa de servicios para separar la logica de negocio de los endpoints"
                    .to_string(),
        });
    }
}

/// Modules with too many dependencies (fan-out > 10).
fn detect_excessive_coupling(graph: &DependencyGraph, out: &mut Vec<AntiPattern>) {
    let metrics = graph.coupling_metrics();
    for (path, (fan_in, fan_out)) in &metrics {
        if *fan_out > 10 {
            out.push(AntiPattern {
                kind: AntiPatternKind::ExcessiveCoupling,
                description: format!("'{}' importa {} modulos (fan-out alto)", path, fan_out),
                affected_modules: vec![path.clone()],
                severity: if *fan_out > 20 {
                    Severity::High
                } else {
                    Severity::Medium
                },
                suggestion: "Reducir dependencias usando inyeccion de dependencias o fachadas"
                    .to_string(),
            });
        }
        let _ = fan_in; // fan_in high = commonly used, not necessarily bad
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_module(path: &str, layer: ArchLayer, loc: usize, funcs: usize) -> ModuleNode {
        ModuleNode {
            path: path.to_string(),
            language: Language::Python,
            layer,
            loc,
            class_count: 1,
            function_count: funcs,
            is_hub: false,
            has_framework_macros: false,
        }
    }

    fn make_module_lang(
        path: &str,
        language: Language,
        layer: ArchLayer,
        loc: usize,
        funcs: usize,
    ) -> ModuleNode {
        ModuleNode {
            path: path.to_string(),
            language,
            layer,
            loc,
            class_count: 1,
            function_count: funcs,
            is_hub: false,
            has_framework_macros: false,
        }
    }

    fn make_module_flags(
        path: &str,
        language: Language,
        loc: usize,
        funcs: usize,
        is_hub: bool,
        has_framework_macros: bool,
    ) -> ModuleNode {
        ModuleNode {
            path: path.to_string(),
            language,
            layer: ArchLayer::Service,
            loc,
            class_count: 1,
            function_count: funcs,
            is_hub,
            has_framework_macros,
        }
    }

    fn make_graph(modules: Vec<ModuleNode>, edges: Vec<ImportEdge>) -> DependencyGraph {
        DependencyGraph {
            root_path: "/project".into(),
            primary_language: Language::Python,
            modules,
            edges,
            external_deps: HashSet::new(),
        }
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Low), "Low");
        assert_eq!(format!("{}", Severity::Medium), "Medium");
        assert_eq!(format!("{}", Severity::High), "High");
    }

    #[test]
    fn test_antipattern_kind_display() {
        assert_eq!(format!("{}", AntiPatternKind::GodClass), "God Class");
        assert_eq!(
            format!("{}", AntiPatternKind::CircularDependency),
            "Circular Dependency"
        );
        assert_eq!(
            format!("{}", AntiPatternKind::FatController),
            "Fat Controller"
        );
        assert_eq!(
            format!("{}", AntiPatternKind::NoServiceLayer),
            "No Service Layer"
        );
        assert_eq!(
            format!("{}", AntiPatternKind::ExcessiveCoupling),
            "Excessive Coupling"
        );
    }

    #[test]
    fn test_detect_god_class_by_loc() {
        let graph = make_graph(
            vec![make_module("big.py", ArchLayer::Service, 600, 5)],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        assert!(results.iter().any(|a| a.kind == AntiPatternKind::GodClass));
    }

    #[test]
    fn test_detect_god_class_by_functions() {
        let graph = make_graph(
            vec![make_module("many.py", ArchLayer::Service, 100, 20)],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        assert!(results.iter().any(|a| a.kind == AntiPatternKind::GodClass));
    }

    #[test]
    fn test_god_class_high_severity() {
        let graph = make_graph(
            vec![make_module("huge.py", ArchLayer::Service, 1200, 30)],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        let god = results
            .iter()
            .find(|a| a.kind == AntiPatternKind::GodClass)
            .unwrap();
        assert_eq!(god.severity, Severity::High);
    }

    #[test]
    fn test_no_god_class_for_tests() {
        let graph = make_graph(
            vec![make_module("test_big.py", ArchLayer::Test, 600, 20)],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        assert!(!results.iter().any(|a| a.kind == AntiPatternKind::GodClass));
    }

    #[test]
    fn test_detect_circular_dependency() {
        let graph = make_graph(
            vec![
                make_module("a.py", ArchLayer::Service, 50, 3),
                make_module("b.py", ArchLayer::Service, 50, 3),
            ],
            vec![
                ImportEdge {
                    from: "a.py".into(),
                    to: "b.py".into(),
                    is_external: false,
                },
                ImportEdge {
                    from: "b.py".into(),
                    to: "a.py".into(),
                    is_external: false,
                },
            ],
        );
        let results = detect_antipatterns(&graph);
        assert!(
            results
                .iter()
                .any(|a| a.kind == AntiPatternKind::CircularDependency)
        );
    }

    #[test]
    fn test_detect_fat_controller_by_loc() {
        let graph = make_graph(
            vec![make_module("ctrl.py", ArchLayer::Controller, 300, 5)],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        assert!(
            results
                .iter()
                .any(|a| a.kind == AntiPatternKind::FatController)
        );
    }

    #[test]
    fn test_detect_fat_controller_imports_repo() {
        let graph = make_graph(
            vec![
                make_module("ctrl.py", ArchLayer::Controller, 50, 3),
                make_module("repo.py", ArchLayer::Repository, 50, 3),
            ],
            vec![ImportEdge {
                from: "ctrl.py".into(),
                to: "repo.py".into(),
                is_external: false,
            }],
        );
        let results = detect_antipatterns(&graph);
        assert!(
            results
                .iter()
                .any(|a| a.kind == AntiPatternKind::FatController)
        );
    }

    #[test]
    fn test_detect_no_service_layer() {
        let mut modules = vec![
            make_module("ctrl.py", ArchLayer::Controller, 50, 3),
            make_module("model.py", ArchLayer::Model, 50, 3),
        ];
        // Need > 5 modules
        for i in 0..5 {
            modules.push(make_module(
                &format!("util{}.py", i),
                ArchLayer::Utility,
                20,
                1,
            ));
        }

        let graph = make_graph(modules, vec![]);
        let results = detect_antipatterns(&graph);
        assert!(
            results
                .iter()
                .any(|a| a.kind == AntiPatternKind::NoServiceLayer)
        );
    }

    #[test]
    fn test_detect_excessive_coupling() {
        let mut modules = vec![make_module("hub.py", ArchLayer::Utility, 50, 3)];
        let mut edges = Vec::new();
        for i in 0..12 {
            let name = format!("dep{}.py", i);
            modules.push(make_module(&name, ArchLayer::Utility, 20, 1));
            edges.push(ImportEdge {
                from: "hub.py".into(),
                to: name,
                is_external: false,
            });
        }

        let graph = make_graph(modules, edges);
        let results = detect_antipatterns(&graph);
        assert!(
            results
                .iter()
                .any(|a| a.kind == AntiPatternKind::ExcessiveCoupling)
        );
    }

    #[test]
    fn test_clean_project_no_antipatterns() {
        let graph = make_graph(
            vec![
                make_module("ctrl.py", ArchLayer::Controller, 50, 3),
                make_module("svc.py", ArchLayer::Service, 80, 5),
                make_module("repo.py", ArchLayer::Repository, 40, 3),
            ],
            vec![
                ImportEdge {
                    from: "ctrl.py".into(),
                    to: "svc.py".into(),
                    is_external: false,
                },
                ImportEdge {
                    from: "svc.py".into(),
                    to: "repo.py".into(),
                    is_external: false,
                },
            ],
        );
        let results = detect_antipatterns(&graph);
        assert!(results.is_empty());
    }

    // ── Language-specific God Class thresholds ──────────────────

    #[test]
    fn test_dart_widget_700loc_20fn_not_god_class() {
        // 700 LOC / 20 functions is below Dart thresholds (800/25)
        let graph = make_graph(
            vec![make_module_lang(
                "lib/screens/marketplace.dart",
                Language::Dart,
                ArchLayer::Controller,
                700,
                20,
            )],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        assert!(
            !results.iter().any(|a| a.kind == AntiPatternKind::GodClass),
            "Dart widget with 700 LOC / 20 fn should NOT be God Class"
        );
    }

    #[test]
    fn test_dart_widget_1600loc_god_class_high() {
        // 1600 LOC exceeds Dart high threshold (1500)
        let graph = make_graph(
            vec![make_module_lang(
                "lib/screens/huge_screen.dart",
                Language::Dart,
                ArchLayer::Controller,
                1600,
                30,
            )],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        let god = results.iter().find(|a| a.kind == AntiPatternKind::GodClass);
        assert!(god.is_some(), "1600 LOC Dart file should be God Class");
        assert_eq!(god.unwrap().severity, Severity::High);
    }

    #[test]
    fn test_python_600loc_still_god_class_medium() {
        // 600 LOC exceeds Python threshold (500) but not high (1000)
        let graph = make_graph(
            vec![make_module_lang(
                "app/services/big.py",
                Language::Python,
                ArchLayer::Service,
                600,
                10,
            )],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        let god = results.iter().find(|a| a.kind == AntiPatternKind::GodClass);
        assert!(god.is_some(), "Python 600 LOC should be God Class");
        assert_eq!(god.unwrap().severity, Severity::Medium);
    }

    #[test]
    fn test_rust_550loc_not_god_class() {
        // 550 LOC is below Rust threshold (600)
        let graph = make_graph(
            vec![make_module_lang(
                "src/analyzer.rs",
                Language::Rust,
                ArchLayer::Service,
                550,
                10,
            )],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        assert!(
            !results.iter().any(|a| a.kind == AntiPatternKind::GodClass),
            "Rust file with 550 LOC should NOT be God Class"
        );
    }

    // ── Fix 2: hub modules ────────────────────────────────────

    #[test]
    fn test_hub_module_not_god_class_even_when_large() {
        // 1500 LOC, 40 functions would normally trip both triggers, but
        // the is_hub flag should suppress the detection entirely.
        let graph = make_graph(
            vec![make_module_flags(
                "src/analyzer/mod.rs",
                Language::Rust,
                1500,
                40,
                true,  // is_hub
                false, // has_framework_macros
            )],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        assert!(
            !results.iter().any(|a| a.kind == AntiPatternKind::GodClass),
            "Re-export hub should never be flagged as God Class"
        );
    }

    // ── Fix 3: framework macro files ──────────────────────────

    #[test]
    fn test_framework_macro_file_ignores_function_count() {
        // 400 LOC (under the Rust threshold of 600) but 50 functions
        // (way over fn_high=35). Normally it would trip on function_count
        // alone — has_framework_macros should suppress that trigger.
        let graph = make_graph(
            vec![make_module_flags(
                "src/mcp/server.rs",
                Language::Rust,
                400,
                50,
                false, // is_hub
                true,  // has_framework_macros
            )],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        assert!(
            !results.iter().any(|a| a.kind == AntiPatternKind::GodClass),
            "Files with #[tool_router] ignore function_count for God Class"
        );
    }

    #[test]
    fn test_framework_macro_file_still_trips_on_loc() {
        // 1500 LOC on a Rust file (loc_high=1200) should still be God Class
        // even when has_framework_macros is true — the LOC check is kept.
        let graph = make_graph(
            vec![make_module_flags(
                "src/mcp/huge_server.rs",
                Language::Rust,
                1500,
                50,
                false,
                true,
            )],
            vec![],
        );
        let results = detect_antipatterns(&graph);
        let god = results
            .iter()
            .find(|a| a.kind == AntiPatternKind::GodClass)
            .expect("framework macro files still trip on LOC");
        assert_eq!(god.severity, Severity::High);
    }
}
