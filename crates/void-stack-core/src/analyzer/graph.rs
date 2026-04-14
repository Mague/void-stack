//! Dependency graph data structures and algorithms.

use std::collections::{HashMap, HashSet};

/// Language of a source module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Python,
    JavaScript,
    TypeScript,
    Go,
    Dart,
    Rust,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Python => write!(f, "Python"),
            Language::JavaScript => write!(f, "JavaScript"),
            Language::TypeScript => write!(f, "TypeScript"),
            Language::Go => write!(f, "Go"),
            Language::Dart => write!(f, "Dart"),
            Language::Rust => write!(f, "Rust"),
        }
    }
}

/// Architectural layer classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArchLayer {
    Controller,
    Service,
    Repository,
    Model,
    Utility,
    Config,
    Test,
    Unknown,
}

impl std::fmt::Display for ArchLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchLayer::Controller => write!(f, "Controller"),
            ArchLayer::Service => write!(f, "Service"),
            ArchLayer::Repository => write!(f, "Repository"),
            ArchLayer::Model => write!(f, "Model"),
            ArchLayer::Utility => write!(f, "Utility"),
            ArchLayer::Config => write!(f, "Config"),
            ArchLayer::Test => write!(f, "Test"),
            ArchLayer::Unknown => write!(f, "Unknown"),
        }
    }
}

/// A single module/file in the dependency graph.
#[derive(Debug, Clone)]
pub struct ModuleNode {
    /// Relative path from project root.
    pub path: String,
    pub language: Language,
    pub layer: ArchLayer,
    /// Lines of code. For Rust, excludes `#[cfg(test)] mod tests { .. }` blocks.
    pub loc: usize,
    /// Number of classes/structs defined.
    pub class_count: usize,
    /// Number of functions/methods defined. For Rust, excludes functions
    /// inside `#[cfg(test)] mod tests { .. }` blocks.
    pub function_count: usize,
    /// Mostly-reexport file (hub). Populated by language parsers; god-class
    /// detection skips hubs entirely.
    pub is_hub: bool,
    /// File contains framework macros that force one function per handler
    /// (currently rmcp `#[tool_router]` / `#[tool_handler]`). God-class
    /// detection ignores function_count for these, keeping only the LOC check.
    pub has_framework_macros: bool,
}

/// An import edge between two modules.
#[derive(Debug, Clone)]
pub struct ImportEdge {
    pub from: String,
    pub to: String,
    pub is_external: bool,
}

/// The complete dependency graph for a project directory.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub root_path: String,
    pub primary_language: Language,
    pub modules: Vec<ModuleNode>,
    pub edges: Vec<ImportEdge>,
    pub external_deps: HashSet<String>,
}

impl DependencyGraph {
    /// Find circular dependencies using Tarjan's SCC algorithm.
    pub fn find_cycles(&self) -> Vec<Vec<String>> {
        let module_paths: Vec<&str> = self.modules.iter().map(|m| m.path.as_str()).collect();
        let mut index_map: HashMap<&str, usize> = HashMap::new();
        for (i, p) in module_paths.iter().enumerate() {
            index_map.insert(p, i);
        }

        // Build adjacency list (internal edges only)
        let n = module_paths.len();
        let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
        for edge in &self.edges {
            if edge.is_external {
                continue;
            }
            if let (Some(&from), Some(&to)) = (
                index_map.get(edge.from.as_str()),
                index_map.get(edge.to.as_str()),
            ) {
                adj[from].push(to);
            }
        }

        // Tarjan's SCC
        let mut index_counter: i32 = 0;
        let mut stack: Vec<usize> = Vec::new();
        let mut on_stack = vec![false; n];
        let mut indices = vec![-1i32; n];
        let mut lowlinks = vec![0i32; n];
        let mut sccs: Vec<Vec<String>> = Vec::new();

        #[allow(clippy::too_many_arguments)]
        fn strongconnect(
            v: usize,
            adj: &[Vec<usize>],
            index_counter: &mut i32,
            stack: &mut Vec<usize>,
            on_stack: &mut [bool],
            indices: &mut [i32],
            lowlinks: &mut [i32],
            sccs: &mut Vec<Vec<String>>,
            paths: &[&str],
        ) {
            indices[v] = *index_counter;
            lowlinks[v] = *index_counter;
            *index_counter += 1;
            stack.push(v);
            on_stack[v] = true;

            for &w in &adj[v] {
                if indices[w] == -1 {
                    strongconnect(
                        w,
                        adj,
                        index_counter,
                        stack,
                        on_stack,
                        indices,
                        lowlinks,
                        sccs,
                        paths,
                    );
                    lowlinks[v] = lowlinks[v].min(lowlinks[w]);
                } else if on_stack[w] {
                    lowlinks[v] = lowlinks[v].min(indices[w]);
                }
            }

            if lowlinks[v] == indices[v] {
                let mut scc = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    scc.push(paths[w].to_string());
                    if w == v {
                        break;
                    }
                }
                if scc.len() > 1 {
                    sccs.push(scc);
                }
            }
        }

        for i in 0..n {
            if indices[i] == -1 {
                strongconnect(
                    i,
                    &adj,
                    &mut index_counter,
                    &mut stack,
                    &mut on_stack,
                    &mut indices,
                    &mut lowlinks,
                    &mut sccs,
                    &module_paths,
                );
            }
        }

        sccs
    }

    /// Compute coupling metrics per module: (fan_in, fan_out).
    pub fn coupling_metrics(&self) -> HashMap<String, (usize, usize)> {
        let mut metrics: HashMap<String, (usize, usize)> = HashMap::new();
        for m in &self.modules {
            metrics.insert(m.path.clone(), (0, 0));
        }
        for edge in &self.edges {
            if edge.is_external {
                continue;
            }
            if let Some(entry) = metrics.get_mut(&edge.from) {
                entry.1 += 1; // fan_out
            }
            if let Some(entry) = metrics.get_mut(&edge.to) {
                entry.0 += 1; // fan_in
            }
        }
        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn make_edge(from: &str, to: &str) -> ImportEdge {
        ImportEdge {
            from: from.into(),
            to: to.into(),
            is_external: false,
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
    fn test_language_display() {
        assert_eq!(format!("{}", Language::Python), "Python");
        assert_eq!(format!("{}", Language::JavaScript), "JavaScript");
        assert_eq!(format!("{}", Language::TypeScript), "TypeScript");
        assert_eq!(format!("{}", Language::Go), "Go");
        assert_eq!(format!("{}", Language::Dart), "Dart");
        assert_eq!(format!("{}", Language::Rust), "Rust");
    }

    #[test]
    fn test_arch_layer_display() {
        assert_eq!(format!("{}", ArchLayer::Controller), "Controller");
        assert_eq!(format!("{}", ArchLayer::Service), "Service");
        assert_eq!(format!("{}", ArchLayer::Repository), "Repository");
        assert_eq!(format!("{}", ArchLayer::Model), "Model");
        assert_eq!(format!("{}", ArchLayer::Utility), "Utility");
        assert_eq!(format!("{}", ArchLayer::Config), "Config");
        assert_eq!(format!("{}", ArchLayer::Test), "Test");
        assert_eq!(format!("{}", ArchLayer::Unknown), "Unknown");
    }

    #[test]
    fn test_find_cycles_no_cycles() {
        let graph = make_graph(
            vec![
                make_module("a.py", ArchLayer::Service, 100, 5),
                make_module("b.py", ArchLayer::Repository, 80, 3),
            ],
            vec![make_edge("a.py", "b.py")],
        );
        assert!(graph.find_cycles().is_empty());
    }

    #[test]
    fn test_find_cycles_with_cycle() {
        let graph = make_graph(
            vec![
                make_module("a.py", ArchLayer::Service, 100, 5),
                make_module("b.py", ArchLayer::Service, 80, 3),
            ],
            vec![make_edge("a.py", "b.py"), make_edge("b.py", "a.py")],
        );
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 2);
    }

    #[test]
    fn test_find_cycles_external_edges_ignored() {
        let graph = make_graph(
            vec![
                make_module("a.py", ArchLayer::Service, 100, 5),
                make_module("b.py", ArchLayer::Service, 80, 3),
            ],
            vec![
                ImportEdge {
                    from: "a.py".into(),
                    to: "b.py".into(),
                    is_external: true,
                },
                ImportEdge {
                    from: "b.py".into(),
                    to: "a.py".into(),
                    is_external: true,
                },
            ],
        );
        assert!(graph.find_cycles().is_empty());
    }

    #[test]
    fn test_coupling_metrics() {
        let graph = make_graph(
            vec![
                make_module("a.py", ArchLayer::Controller, 50, 3),
                make_module("b.py", ArchLayer::Service, 50, 3),
                make_module("c.py", ArchLayer::Repository, 50, 3),
            ],
            vec![
                make_edge("a.py", "b.py"),
                make_edge("a.py", "c.py"),
                make_edge("b.py", "c.py"),
            ],
        );
        let metrics = graph.coupling_metrics();
        // a: fan_in=0, fan_out=2
        assert_eq!(metrics["a.py"], (0, 2));
        // b: fan_in=1, fan_out=1
        assert_eq!(metrics["b.py"], (1, 1));
        // c: fan_in=2, fan_out=0
        assert_eq!(metrics["c.py"], (2, 0));
    }

    #[test]
    fn test_coupling_metrics_external_ignored() {
        let graph = make_graph(
            vec![make_module("a.py", ArchLayer::Service, 50, 3)],
            vec![ImportEdge {
                from: "a.py".into(),
                to: "flask".into(),
                is_external: true,
            }],
        );
        let metrics = graph.coupling_metrics();
        assert_eq!(metrics["a.py"], (0, 0));
    }
}
