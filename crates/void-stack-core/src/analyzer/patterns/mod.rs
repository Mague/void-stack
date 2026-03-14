//! Architecture pattern detection and anti-pattern analysis.

pub mod antipatterns;

use std::collections::HashMap;

use super::graph::*;

/// Detected architecture pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum ArchPattern {
    Mvc,
    CleanHexagonal,
    Layered,
    Microservices,
    Monolith,
    Unknown,
}

impl std::fmt::Display for ArchPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchPattern::Mvc => write!(f, "MVC"),
            ArchPattern::CleanHexagonal => write!(f, "Clean / Hexagonal"),
            ArchPattern::Layered => write!(f, "Layered"),
            ArchPattern::Microservices => write!(f, "Microservices"),
            ArchPattern::Monolith => write!(f, "Monolith"),
            ArchPattern::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Full architecture analysis result.
#[derive(Debug, Clone)]
pub struct ArchAnalysis {
    pub detected_pattern: ArchPattern,
    pub confidence: f32,
    pub layer_distribution: HashMap<ArchLayer, usize>,
    pub anti_patterns: Vec<antipatterns::AntiPattern>,
}

/// Detect architecture pattern from the dependency graph.
pub fn detect_architecture(graph: &DependencyGraph) -> ArchAnalysis {
    let mut layer_dist: HashMap<ArchLayer, usize> = HashMap::new();
    for m in &graph.modules {
        *layer_dist.entry(m.layer).or_insert(0) += 1;
    }

    let has_controllers = layer_dist.get(&ArchLayer::Controller).copied().unwrap_or(0) > 0;
    let has_services = layer_dist.get(&ArchLayer::Service).copied().unwrap_or(0) > 0;
    let has_repos = layer_dist.get(&ArchLayer::Repository).copied().unwrap_or(0) > 0;
    let has_models = layer_dist.get(&ArchLayer::Model).copied().unwrap_or(0) > 0;

    // Detect anti-patterns
    let antis = antipatterns::detect_antipatterns(graph);

    // Scoring
    let (pattern, confidence) = if has_controllers && has_services && has_repos && has_models {
        // Check if it's Clean/Hexagonal (domain has no outward deps)
        let is_clean = check_clean_architecture(graph);
        if is_clean {
            (ArchPattern::CleanHexagonal, 0.85)
        } else {
            (ArchPattern::Layered, 0.8)
        }
    } else if has_controllers && has_models && !has_services {
        (ArchPattern::Mvc, 0.75)
    } else if has_controllers && has_services {
        (ArchPattern::Layered, 0.7)
    } else if graph.modules.len() > 20 && !has_controllers && !has_services {
        (ArchPattern::Monolith, 0.6)
    } else if graph.modules.len() <= 5 {
        (ArchPattern::Monolith, 0.5)
    } else {
        (ArchPattern::Unknown, 0.3)
    };

    ArchAnalysis {
        detected_pattern: pattern,
        confidence,
        layer_distribution: layer_dist,
        anti_patterns: antis,
    }
}

/// Check if the architecture follows Clean/Hexagonal principles.
/// Domain/Service modules should not import from Controller or Repository layers.
#[allow(dead_code)]
fn check_clean_architecture(graph: &DependencyGraph) -> bool {
    let module_layers: HashMap<&str, ArchLayer> = graph
        .modules
        .iter()
        .map(|m| (m.path.as_str(), m.layer))
        .collect();

    for edge in &graph.edges {
        if edge.is_external {
            continue;
        }
        let from_layer = module_layers.get(edge.from.as_str());
        let to_layer = module_layers.get(edge.to.as_str());

        if let (Some(ArchLayer::Service), Some(ArchLayer::Controller)) = (from_layer, to_layer) {
            return false; // Service should not depend on Controller
        }
        if let (Some(ArchLayer::Model), Some(ArchLayer::Controller)) = (from_layer, to_layer) {
            return false; // Model should not depend on Controller
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_module(path: &str, layer: ArchLayer) -> ModuleNode {
        ModuleNode {
            path: path.to_string(),
            language: Language::Python,
            layer,
            loc: 100,
            class_count: 1,
            function_count: 5,
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
    fn test_detect_mvc() {
        let graph = make_graph(
            vec![
                make_module("controllers/user.py", ArchLayer::Controller),
                make_module("models/user.py", ArchLayer::Model),
                make_module("views/home.py", ArchLayer::Utility),
            ],
            vec![],
        );
        let result = detect_architecture(&graph);
        assert_eq!(result.detected_pattern, ArchPattern::Mvc);
    }

    #[test]
    fn test_detect_layered() {
        let graph = make_graph(
            vec![
                make_module("controllers/api.py", ArchLayer::Controller),
                make_module("services/auth.py", ArchLayer::Service),
                make_module("repos/user_repo.py", ArchLayer::Repository),
                make_module("models/user.py", ArchLayer::Model),
            ],
            vec![ImportEdge {
                from: "services/auth.py".into(),
                to: "controllers/api.py".into(),
                is_external: false,
            }],
        );
        let result = detect_architecture(&graph);
        // Service depends on Controller -> not clean, so Layered
        assert_eq!(result.detected_pattern, ArchPattern::Layered);
        assert!(result.confidence >= 0.7);
    }

    #[test]
    fn test_detect_clean_hexagonal() {
        let graph = make_graph(
            vec![
                make_module("controllers/api.py", ArchLayer::Controller),
                make_module("services/auth.py", ArchLayer::Service),
                make_module("repos/user_repo.py", ArchLayer::Repository),
                make_module("models/user.py", ArchLayer::Model),
            ],
            vec![
                ImportEdge {
                    from: "controllers/api.py".into(),
                    to: "services/auth.py".into(),
                    is_external: false,
                },
                ImportEdge {
                    from: "services/auth.py".into(),
                    to: "repos/user_repo.py".into(),
                    is_external: false,
                },
            ],
        );
        let result = detect_architecture(&graph);
        assert_eq!(result.detected_pattern, ArchPattern::CleanHexagonal);
        assert!(result.confidence >= 0.8);
    }

    #[test]
    fn test_detect_monolith_small() {
        let graph = make_graph(
            vec![
                make_module("main.py", ArchLayer::Utility),
                make_module("utils.py", ArchLayer::Utility),
            ],
            vec![],
        );
        let result = detect_architecture(&graph);
        assert_eq!(result.detected_pattern, ArchPattern::Monolith);
    }

    #[test]
    fn test_detect_unknown() {
        let modules: Vec<ModuleNode> = (0..10)
            .map(|i| make_module(&format!("mod{}.py", i), ArchLayer::Utility))
            .collect();
        let graph = make_graph(modules, vec![]);
        let result = detect_architecture(&graph);
        assert_eq!(result.detected_pattern, ArchPattern::Unknown);
    }

    #[test]
    fn test_layer_distribution() {
        let graph = make_graph(
            vec![
                make_module("ctrl1.py", ArchLayer::Controller),
                make_module("ctrl2.py", ArchLayer::Controller),
                make_module("svc1.py", ArchLayer::Service),
                make_module("model1.py", ArchLayer::Model),
            ],
            vec![],
        );
        let result = detect_architecture(&graph);
        assert_eq!(
            *result
                .layer_distribution
                .get(&ArchLayer::Controller)
                .unwrap(),
            2
        );
        assert_eq!(
            *result.layer_distribution.get(&ArchLayer::Service).unwrap(),
            1
        );
    }

    #[test]
    fn test_arch_pattern_display() {
        assert_eq!(format!("{}", ArchPattern::Mvc), "MVC");
        assert_eq!(
            format!("{}", ArchPattern::CleanHexagonal),
            "Clean / Hexagonal"
        );
        assert_eq!(format!("{}", ArchPattern::Layered), "Layered");
        assert_eq!(format!("{}", ArchPattern::Monolith), "Monolith");
        assert_eq!(format!("{}", ArchPattern::Unknown), "Unknown");
    }
}
