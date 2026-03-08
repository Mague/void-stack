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

/// Files with too many functions or too many LOC.
fn detect_god_classes(graph: &DependencyGraph, out: &mut Vec<AntiPattern>) {
    for m in &graph.modules {
        if m.layer == ArchLayer::Test {
            continue;
        }
        let is_god = m.loc > 500 || m.function_count > 15;
        if is_god {
            let reason = if m.loc > 500 && m.function_count > 15 {
                format!("{} LOC y {} funciones", m.loc, m.function_count)
            } else if m.loc > 500 {
                format!("{} LOC", m.loc)
            } else {
                format!("{} funciones", m.function_count)
            };
            out.push(AntiPattern {
                kind: AntiPatternKind::GodClass,
                description: format!("'{}' es demasiado grande ({})", m.path, reason),
                affected_modules: vec![m.path.clone()],
                severity: if m.loc > 1000 || m.function_count > 25 { Severity::High } else { Severity::Medium },
                suggestion: format!("Dividir '{}' en modulos mas pequenos con responsabilidades claras", m.path),
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
            suggestion: "Extraer la interfaz comun a un modulo separado o invertir la dependencia".to_string(),
        });
    }
}

/// Controllers with too much logic (high LOC or importing repositories directly).
fn detect_fat_controllers(graph: &DependencyGraph, out: &mut Vec<AntiPattern>) {
    let module_layers: std::collections::HashMap<&str, ArchLayer> = graph.modules.iter()
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
                description: format!("Controller '{}' tiene {} LOC — demasiada logica", m.path, m.loc),
                affected_modules: vec![m.path.clone()],
                severity: if m.loc > 400 { Severity::High } else { Severity::Medium },
                suggestion: "Mover la logica de negocio a una capa de servicio".to_string(),
            });
        }

        // Check if controller imports repository directly
        for edge in &graph.edges {
            if edge.from == m.path && !edge.is_external {
                if let Some(ArchLayer::Repository) = module_layers.get(edge.to.as_str()) {
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
}

/// Controllers without a service layer.
fn detect_no_service_layer(graph: &DependencyGraph, out: &mut Vec<AntiPattern>) {
    let has_controllers = graph.modules.iter().any(|m| m.layer == ArchLayer::Controller);
    let has_services = graph.modules.iter().any(|m| m.layer == ArchLayer::Service);

    if has_controllers && !has_services && graph.modules.len() > 5 {
        let controllers: Vec<String> = graph.modules.iter()
            .filter(|m| m.layer == ArchLayer::Controller)
            .map(|m| m.path.clone())
            .collect();

        out.push(AntiPattern {
            kind: AntiPatternKind::NoServiceLayer,
            description: format!("Proyecto tiene {} controllers pero ninguna capa de servicio", controllers.len()),
            affected_modules: controllers,
            severity: Severity::Medium,
            suggestion: "Crear una capa de servicios para separar la logica de negocio de los endpoints".to_string(),
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
                severity: if *fan_out > 20 { Severity::High } else { Severity::Medium },
                suggestion: "Reducir dependencias usando inyeccion de dependencias o fachadas".to_string(),
            });
        }
        let _ = fan_in; // fan_in high = commonly used, not necessarily bad
    }
}
