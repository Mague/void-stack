//! Prompt builder: converts analysis results into a focused context for the LLM.

use crate::analyzer::AnalysisResult;
use crate::analyzer::patterns::antipatterns::AntiPatternKind;

/// A code chunk retrieved from the semantic index to enrich the prompt.
#[derive(Debug, Clone)]
pub struct CodeContext {
    /// The hotspot label (function name, module path, etc.)
    pub label: String,
    /// Relevant code chunks (top-3 from semantic search)
    pub chunks: Vec<String>,
}

/// Build an optimized prompt from analysis results.
///
/// Keeps the prompt concise — metadata only, no code dumps.
/// Prompt is in Spanish since the user prefers it.
pub fn build_prompt(analysis: &AnalysisResult, project_name: &str) -> String {
    build_prompt_with_context(analysis, project_name, &[])
}

/// Build a prompt enriched with code context from the semantic index.
///
/// If `code_contexts` is empty, behaves identically to `build_prompt`.
pub fn build_prompt_with_context(
    analysis: &AnalysisResult,
    project_name: &str,
    code_contexts: &[CodeContext],
) -> String {
    let mut sections = Vec::new();

    // Header
    sections.push(format!(
        "Eres un experto en arquitectura de software. Analiza los siguientes datos del proyecto \
         '{}' y genera sugerencias concretas y accionables para mejorar la calidad del código.\n",
        project_name
    ));

    // Architecture pattern
    sections.push(format!(
        "## Patrón arquitectónico detectado\n{} (confianza: {:.0}%)",
        analysis.architecture.detected_pattern,
        analysis.architecture.confidence * 100.0,
    ));

    // Layer distribution
    if !analysis.architecture.layer_distribution.is_empty() {
        let mut layer_lines: Vec<String> = analysis
            .architecture
            .layer_distribution
            .iter()
            .map(|(layer, count)| format!("- {}: {}", layer, count))
            .collect();
        layer_lines.sort();
        sections.push(format!(
            "## Distribución de capas\n{}",
            layer_lines.join("\n")
        ));
    }

    // Module stats
    let total_modules = analysis.graph.modules.len();
    let total_loc: usize = analysis.graph.modules.iter().map(|m| m.loc).sum();
    sections.push(format!(
        "## Métricas generales\n- Módulos: {}\n- Líneas de código total: {}",
        total_modules, total_loc,
    ));

    // Anti-patterns
    if !analysis.architecture.anti_patterns.is_empty() {
        let mut ap_lines = Vec::new();
        for ap in &analysis.architecture.anti_patterns {
            let kind_label = match ap.kind {
                AntiPatternKind::GodClass => "God Class",
                AntiPatternKind::CircularDependency => "Dependencia circular",
                AntiPatternKind::FatController => "Fat Controller",
                AntiPatternKind::NoServiceLayer => "Sin capa de servicio",
                AntiPatternKind::ExcessiveCoupling => "Acoplamiento excesivo",
            };
            ap_lines.push(format!(
                "- **{}** [{}]: {} (archivos: {})",
                kind_label,
                ap.severity,
                ap.description,
                ap.affected_modules.join(", "),
            ));
        }
        sections.push(format!(
            "## Anti-patrones detectados ({})\n{}",
            analysis.architecture.anti_patterns.len(),
            ap_lines.join("\n"),
        ));
    }

    // Complexity hotspots
    if let Some(ref complexity) = analysis.complexity {
        let mut hot: Vec<(String, String, usize, usize)> = Vec::new();
        for (file, fc) in complexity {
            for func in fc.complex_functions(8) {
                hot.push((file.clone(), func.name.clone(), func.line, func.complexity));
            }
        }
        hot.sort_by_key(|x| std::cmp::Reverse(x.3));
        hot.truncate(15);

        if !hot.is_empty() {
            let lines: Vec<String> = hot
                .iter()
                .map(|(f, name, line, cx)| {
                    format!("- `{}:{}` → {}() — complejidad {}", f, line, name, cx)
                })
                .collect();
            sections.push(format!(
                "## Funciones más complejas (top {})\n{}",
                hot.len(),
                lines.join("\n"),
            ));
        }
    }

    // Circular dependencies
    let cycles = analysis.graph.find_cycles();
    if !cycles.is_empty() {
        let cycle_lines: Vec<String> = cycles
            .iter()
            .map(|c| format!("- {}", c.join(" <-> ")))
            .collect();
        sections.push(format!(
            "## Dependencias circulares ({})\n{}",
            cycles.len(),
            cycle_lines.join("\n"),
        ));
    }

    // Coverage
    if let Some(ref cov) = analysis.coverage {
        sections.push(format!(
            "## Cobertura de tests\n- {:.1}% ({} de {} líneas cubiertas, herramienta: {})",
            cov.coverage_percent, cov.covered_lines, cov.total_lines, cov.tool,
        ));
    } else {
        sections
            .push("## Cobertura de tests\nNo se encontraron reportes de cobertura.".to_string());
    }

    // Code context from semantic index (if available)
    if !code_contexts.is_empty() {
        let mut ctx_lines = Vec::new();
        for ctx in code_contexts {
            ctx_lines.push(format!("### {}", ctx.label));
            for (i, chunk) in ctx.chunks.iter().enumerate() {
                ctx_lines.push(format!("**Fragmento {}:**\n```\n{}\n```", i + 1, chunk));
            }
        }
        sections.push(format!(
            "## Contexto de código relevante\nCódigo real de los hotspots detectados (obtenido del índice semántico):\n\n{}",
            ctx_lines.join("\n\n"),
        ));
    }

    // Instructions for the LLM
    sections.push(
        "## Instrucciones\n\
         Genera una lista numerada de sugerencias concretas. Para cada sugerencia incluye:\n\
         1. Un título breve y descriptivo\n\
         2. Descripción del problema y por qué es importante\n\
         3. Los archivos afectados (usa rutas como `path/to/file.ext`)\n\
         4. Pasos concretos para resolverlo\n\
         5. Prioridad: Critical, High, Medium o Low\n\n\
         Enfócate en:\n\
         - Refactorizaciones que reduzcan la complejidad\n\
         - Mejoras arquitectónicas basadas en el patrón detectado\n\
         - Eliminación de anti-patrones\n\
         - Mejoras de rendimiento si hay señales claras\n\
         - Problemas de seguridad si los detectas en la estructura\n\n\
         Responde en español. Sé específico con las rutas de archivo."
            .to_string(),
    );

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::AnalysisResult;
    use crate::analyzer::graph::ArchLayer;
    use crate::analyzer::graph::DependencyGraph;
    use crate::analyzer::patterns::{ArchAnalysis, ArchPattern};
    use std::collections::HashMap;

    fn dummy_analysis() -> AnalysisResult {
        let mut layer_dist = HashMap::new();
        layer_dist.insert(ArchLayer::Controller, 3);
        layer_dist.insert(ArchLayer::Service, 2);

        AnalysisResult {
            graph: DependencyGraph {
                root_path: String::new(),
                primary_language: crate::analyzer::graph::Language::Python,
                modules: vec![],
                edges: vec![],
                external_deps: std::collections::HashSet::new(),
            },
            architecture: ArchAnalysis {
                detected_pattern: ArchPattern::Layered,
                confidence: 0.8,
                layer_distribution: layer_dist,
                anti_patterns: vec![],
            },
            coverage: None,
            complexity: None,
            explicit_debt: vec![],
        }
    }

    #[test]
    fn test_build_prompt_basic() {
        let analysis = dummy_analysis();
        let prompt = build_prompt(&analysis, "test-project");
        assert!(prompt.contains("test-project"));
        assert!(prompt.contains("Layered"));
        assert!(prompt.contains("80%"));
        assert!(prompt.contains("Instrucciones"));
        assert!(prompt.contains("español"));
    }

    #[test]
    fn test_build_prompt_with_antipatterns() {
        let mut analysis = dummy_analysis();
        analysis.architecture.anti_patterns.push(
            crate::analyzer::patterns::antipatterns::AntiPattern {
                kind: AntiPatternKind::GodClass,
                description: "server.rs es demasiado grande".to_string(),
                affected_modules: vec!["src/server.rs".to_string()],
                severity: crate::analyzer::patterns::antipatterns::Severity::High,
                suggestion: "Dividir en módulos".to_string(),
            },
        );
        let prompt = build_prompt(&analysis, "test");
        assert!(prompt.contains("God Class"));
        assert!(prompt.contains("server.rs"));
    }

    #[test]
    fn test_build_prompt_with_coverage() {
        let mut analysis = dummy_analysis();
        analysis.coverage = Some(crate::analyzer::coverage::CoverageData {
            tool: "pytest-cov".into(),
            total_lines: 500,
            covered_lines: 350,
            coverage_percent: 70.0,
            files: vec![],
        });
        let prompt = build_prompt(&analysis, "test");
        assert!(prompt.contains("70.0%"));
        assert!(prompt.contains("pytest-cov"));
    }

    #[test]
    fn test_build_prompt_no_coverage() {
        let analysis = dummy_analysis();
        let prompt = build_prompt(&analysis, "test");
        assert!(prompt.contains("No se encontraron reportes de cobertura"));
    }

    #[test]
    fn test_build_prompt_with_complexity() {
        let mut analysis = dummy_analysis();
        analysis.complexity = Some(vec![(
            "handlers.py".into(),
            crate::analyzer::complexity::FileComplexity {
                functions: vec![crate::analyzer::complexity::FunctionComplexity {
                    name: "process_request".into(),
                    line: 42,
                    complexity: 15,
                    loc: 80,
                    has_coverage: None,
                }],
            },
        )]);
        let prompt = build_prompt(&analysis, "test");
        assert!(prompt.contains("process_request"));
        assert!(prompt.contains("complejidad 15"));
    }

    #[test]
    fn test_build_prompt_with_circular_deps() {
        let mut analysis = dummy_analysis();
        analysis
            .graph
            .modules
            .push(crate::analyzer::graph::ModuleNode {
                path: "a.py".into(),
                language: crate::analyzer::graph::Language::Python,
                layer: ArchLayer::Service,
                loc: 50,
                class_count: 0,
                function_count: 3,
                is_hub: false,
                has_framework_macros: false,
            });
        analysis
            .graph
            .modules
            .push(crate::analyzer::graph::ModuleNode {
                path: "b.py".into(),
                language: crate::analyzer::graph::Language::Python,
                layer: ArchLayer::Service,
                loc: 50,
                class_count: 0,
                function_count: 3,
                is_hub: false,
                has_framework_macros: false,
            });
        analysis
            .graph
            .edges
            .push(crate::analyzer::graph::ImportEdge {
                from: "a.py".into(),
                to: "b.py".into(),
                is_external: false,
            });
        analysis
            .graph
            .edges
            .push(crate::analyzer::graph::ImportEdge {
                from: "b.py".into(),
                to: "a.py".into(),
                is_external: false,
            });
        let prompt = build_prompt(&analysis, "test");
        assert!(prompt.contains("Dependencias circulares"));
    }

    #[test]
    fn test_build_prompt_all_antipattern_kinds() {
        let mut analysis = dummy_analysis();
        let kinds = [
            (AntiPatternKind::GodClass, "God Class"),
            (AntiPatternKind::CircularDependency, "Dependencia circular"),
            (AntiPatternKind::FatController, "Fat Controller"),
            (AntiPatternKind::NoServiceLayer, "Sin capa de servicio"),
            (AntiPatternKind::ExcessiveCoupling, "Acoplamiento excesivo"),
        ];
        for (kind, _label) in &kinds {
            analysis.architecture.anti_patterns.push(
                crate::analyzer::patterns::antipatterns::AntiPattern {
                    kind: *kind,
                    description: "test".into(),
                    affected_modules: vec!["x.py".into()],
                    severity: crate::analyzer::patterns::antipatterns::Severity::Medium,
                    suggestion: "fix".into(),
                },
            );
        }
        let prompt = build_prompt(&analysis, "test");
        for (_, label) in &kinds {
            assert!(prompt.contains(label), "Should contain: {}", label);
        }
    }

    #[test]
    fn test_build_prompt_with_code_context() {
        let analysis = dummy_analysis();
        let contexts = vec![CodeContext {
            label: "handlers.py:process_request() — CC 15".to_string(),
            chunks: vec![
                "def process_request(req):\n    if req.method == 'POST':".to_string(),
                "    return response".to_string(),
            ],
        }];
        let prompt = build_prompt_with_context(&analysis, "test", &contexts);
        assert!(prompt.contains("Contexto de código relevante"));
        assert!(prompt.contains("handlers.py:process_request()"));
        assert!(prompt.contains("def process_request"));
        assert!(prompt.contains("Fragmento 1:"));
        assert!(prompt.contains("Fragmento 2:"));
    }

    #[test]
    fn test_build_prompt_without_code_context_same_as_basic() {
        let analysis = dummy_analysis();
        let basic = build_prompt(&analysis, "test");
        let with_empty = build_prompt_with_context(&analysis, "test", &[]);
        assert_eq!(basic, with_empty);
    }
}
