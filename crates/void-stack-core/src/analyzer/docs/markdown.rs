//! Markdown documentation generation from analysis results.

use std::collections::HashMap;

use crate::analyzer::AnalysisResult;
use crate::analyzer::graph::*;
use crate::analyzer::patterns::antipatterns::Severity;

use super::coverage::coverage_hint;
use super::sanitize::sanitize_id;

/// Generate a full markdown architecture document.
pub fn generate_docs(result: &AnalysisResult, project_name: &str) -> String {
    let mut md = String::new();

    write_header(&mut md, result, project_name);
    write_layer_distribution(&mut md, result);
    write_anti_patterns(&mut md, result);
    write_dependency_map(&mut md, result);
    write_modules_table(&mut md, result);
    write_external_deps(&mut md, result);
    write_complexity(&mut md, result);
    write_coupling_metrics(&mut md, result);
    write_coverage(&mut md, result);
    write_explicit_debt(&mut md, result);

    md.push_str("---\n*Generado automaticamente por VoidStack*\n");

    md
}

fn write_header(md: &mut String, result: &AnalysisResult, project_name: &str) {
    md.push_str(&format!("# Arquitectura: {}\n\n", project_name));
    md.push_str("## Resumen\n\n");
    md.push_str("| | |\n|---|---|\n");
    md.push_str(&format!(
        "| **Patron** | {} (confianza: {:.0}%) |\n",
        result.architecture.detected_pattern,
        result.architecture.confidence * 100.0
    ));
    md.push_str(&format!(
        "| **Lenguaje** | {} |\n",
        result.graph.primary_language
    ));
    md.push_str(&format!(
        "| **Modulos** | {} archivos |\n",
        result.graph.modules.len()
    ));

    let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();
    md.push_str(&format!("| **LOC** | {} lineas |\n", total_loc));
    md.push_str(&format!(
        "| **Deps externas** | {} paquetes |\n",
        result.graph.external_deps.len()
    ));
    md.push('\n');
}

fn write_layer_distribution(md: &mut String, result: &AnalysisResult) {
    md.push_str("## Distribucion por Capas\n\n");
    md.push_str("| Capa | Archivos | LOC | % |\n");
    md.push_str("|------|----------|-----|---|\n");

    let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();
    let mut layer_loc: HashMap<ArchLayer, usize> = HashMap::new();
    for m in &result.graph.modules {
        *layer_loc.entry(m.layer).or_insert(0) += m.loc;
    }

    for layer in &LAYERS_ORDER {
        let count = result
            .architecture
            .layer_distribution
            .get(layer)
            .copied()
            .unwrap_or(0);
        if count == 0 {
            continue;
        }
        let loc = layer_loc.get(layer).copied().unwrap_or(0);
        let pct = if total_loc > 0 {
            (loc as f32 / total_loc as f32 * 100.0) as u32
        } else {
            0
        };
        md.push_str(&format!("| {} | {} | {} | {}% |\n", layer, count, loc, pct));
    }
    md.push('\n');
}

fn write_anti_patterns(md: &mut String, result: &AnalysisResult) {
    if !result.architecture.anti_patterns.is_empty() {
        md.push_str("## Anti-patrones Detectados\n\n");

        let high: Vec<_> = result
            .architecture
            .anti_patterns
            .iter()
            .filter(|a| a.severity == Severity::High)
            .collect();
        let medium: Vec<_> = result
            .architecture
            .anti_patterns
            .iter()
            .filter(|a| a.severity == Severity::Medium)
            .collect();
        let low: Vec<_> = result
            .architecture
            .anti_patterns
            .iter()
            .filter(|a| a.severity == Severity::Low)
            .collect();

        write_severity_group(md, "Alta Severidad", &high);
        write_severity_group(md, "Severidad Media", &medium);
        write_severity_group(md, "Baja Severidad", &low);
    } else {
        md.push_str("## Anti-patrones\n\nNo se detectaron anti-patrones significativos.\n\n");
    }
}

fn write_severity_group(
    md: &mut String,
    title: &str,
    patterns: &[&crate::analyzer::patterns::antipatterns::AntiPattern],
) {
    if !patterns.is_empty() {
        md.push_str(&format!("### {}\n\n", title));
        for ap in patterns {
            md.push_str(&format!("- **{}**: {}\n", ap.kind, ap.description));
            md.push_str(&format!("  - *Sugerencia*: {}\n", ap.suggestion));
        }
        md.push('\n');
    }
}

fn write_dependency_map(md: &mut String, result: &AnalysisResult) {
    md.push_str("## Mapa de Dependencias\n\n");
    md.push_str("```mermaid\ngraph LR\n");

    for layer in &LAYERS_ORDER {
        let modules_in_layer: Vec<_> = result
            .graph
            .modules
            .iter()
            .filter(|m| m.layer == *layer)
            .collect();
        if modules_in_layer.is_empty() {
            continue;
        }
        let layer_id = format!("{:?}", layer).to_lowercase();
        md.push_str(&format!("    subgraph {} [\"{}\"]\n", layer_id, layer));
        for m in &modules_in_layer {
            let id = sanitize_id(&m.path);
            let short_name = m.path.rsplit('/').next().unwrap_or(&m.path);
            md.push_str(&format!("        {}[\"{}\"]\n", id, short_name));
        }
        md.push_str("    end\n");
    }

    let internal_edges: Vec<_> = result
        .graph
        .edges
        .iter()
        .filter(|e| !e.is_external)
        .filter(|e| result.graph.modules.iter().any(|m| m.path == e.to))
        .collect();

    let max_edges = 50;
    for edge in internal_edges.iter().take(max_edges) {
        let from_id = sanitize_id(&edge.from);
        let to_id = sanitize_id(&edge.to);
        md.push_str(&format!("    {} --> {}\n", from_id, to_id));
    }
    if internal_edges.len() > max_edges {
        md.push_str(&format!(
            "    %% ... y {} conexiones mas\n",
            internal_edges.len() - max_edges
        ));
    }

    md.push_str("```\n\n");
}

const MAX_MODULE_ROWS: usize = 30;

fn write_modules_table(md: &mut String, result: &AnalysisResult) {
    md.push_str("## Modulos\n\n");
    md.push_str("| Archivo | Capa | LOC | Clases | Funciones |\n");
    md.push_str("|---------|------|-----|--------|----------|\n");

    let mut sorted_modules: Vec<_> = result.graph.modules.iter().collect();
    sorted_modules.sort_by(|a, b| b.loc.cmp(&a.loc));

    let total = sorted_modules.len();

    for m in sorted_modules.iter().take(MAX_MODULE_ROWS) {
        md.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            m.path, m.layer, m.loc, m.class_count, m.function_count
        ));
    }

    if total > MAX_MODULE_ROWS {
        md.push_str(&format!(
            "\n*... y {} módulos más (ordenados por LOC, mostrando top {})*\n",
            total - MAX_MODULE_ROWS,
            MAX_MODULE_ROWS
        ));
    }
    md.push('\n');
}

fn write_external_deps(md: &mut String, result: &AnalysisResult) {
    if !result.graph.external_deps.is_empty() {
        md.push_str("## Dependencias Externas\n\n");
        let mut deps: Vec<_> = result.graph.external_deps.iter().collect();
        deps.sort();
        for dep in &deps {
            md.push_str(&format!("- `{}`\n", dep));
        }
        md.push('\n');
    }
}

fn write_complexity(md: &mut String, result: &AnalysisResult) {
    if let Some(cx_data) = &result.complexity {
        md.push_str("## Complejidad Ciclomatica\n\n");

        let all_funcs: Vec<_> = cx_data
            .iter()
            .flat_map(|(path, fc)| fc.functions.iter().map(move |f| (path.as_str(), f)))
            .collect();

        if !all_funcs.is_empty() {
            let total_cx: usize = all_funcs.iter().map(|(_, f)| f.complexity).sum();
            let avg_cx = total_cx as f32 / all_funcs.len() as f32;
            let complex_count = all_funcs.iter().filter(|(_, f)| f.complexity >= 10).count();

            md.push_str(&format!("**Promedio**: {:.1} | **Funciones analizadas**: {} | **Funciones complejas (>=10)**: {}\n\n",
                avg_cx, all_funcs.len(), complex_count));

            let mut sorted: Vec<_> = all_funcs
                .iter()
                .filter(|(_, f)| f.complexity >= 5)
                .collect();
            sorted.sort_by(|a, b| b.1.complexity.cmp(&a.1.complexity));

            if !sorted.is_empty() {
                let has_any_coverage = sorted.iter().any(|(_, f)| f.has_coverage.is_some());
                if has_any_coverage {
                    md.push_str("| Funcion | Archivo | Linea | CC | LOC | Cobertura |\n");
                    md.push_str("|---------|---------|-------|----|-----|----------|\n");
                } else {
                    md.push_str("| Funcion | Archivo | Linea | CC | LOC |\n");
                    md.push_str("|---------|---------|-------|----|-----|\n");
                }

                for (path, func) in sorted.iter().take(20) {
                    let icon = if func.complexity >= 15 {
                        "!!"
                    } else if func.complexity >= 10 {
                        "!"
                    } else {
                        ""
                    };
                    let short = path.rsplit('/').next().unwrap_or(path);
                    if has_any_coverage {
                        let cov_icon = match func.has_coverage {
                            Some(true) => "✅",
                            Some(false) => "🔴",
                            None => "-",
                        };
                        md.push_str(&format!(
                            "| `{}` {} | `{}` | {} | {} | {} | {} |\n",
                            func.name, icon, short, func.line, func.complexity, func.loc, cov_icon
                        ));
                    } else {
                        md.push_str(&format!(
                            "| `{}` {} | `{}` | {} | {} | {} |\n",
                            func.name, icon, short, func.line, func.complexity, func.loc
                        ));
                    }
                }
                md.push('\n');
            }
        }
    }
}

fn write_coupling_metrics(md: &mut String, result: &AnalysisResult) {
    md.push_str("## Metricas de Acoplamiento\n\n");
    md.push_str("| Modulo | Fan-in | Fan-out |\n");
    md.push_str("|--------|--------|--------|\n");

    let metrics = result.graph.coupling_metrics();
    let mut metric_list: Vec<_> = metrics.iter().collect();
    metric_list.sort_by(|a, b| b.1.1.cmp(&a.1.1));

    for (path, (fan_in, fan_out)) in metric_list.iter().take(20) {
        if *fan_in + *fan_out == 0 {
            continue;
        }
        let short = path.rsplit('/').next().unwrap_or(path);
        md.push_str(&format!("| `{}` | {} | {} |\n", short, fan_in, fan_out));
    }
    md.push('\n');
}

fn write_coverage(md: &mut String, result: &AnalysisResult) {
    if result.coverage.is_none() {
        let hint = coverage_hint(&result.graph);
        if let Some(hint_text) = hint {
            md.push_str("## Test Coverage\n\n");
            md.push_str("⚠️ No se encontraron reportes de cobertura.\n\n");
            md.push_str(&hint_text);
            md.push_str("\n\n");
        }
    }
    if let Some(cov) = &result.coverage {
        md.push_str("## Test Coverage\n\n");
        md.push_str(&format!(
            "**Herramienta**: {} | **Cobertura total**: {:.1}% ({}/{} lineas)\n\n",
            cov.tool, cov.coverage_percent, cov.covered_lines, cov.total_lines
        ));

        let filled = (cov.coverage_percent / 5.0) as usize;
        let empty = 20_usize.saturating_sub(filled);
        let bar_color = if cov.coverage_percent >= 80.0 {
            "🟢"
        } else if cov.coverage_percent >= 50.0 {
            "🟡"
        } else {
            "🔴"
        };
        md.push_str(&format!(
            "{} `[{}{}]` {:.1}%\n\n",
            bar_color,
            "█".repeat(filled),
            "░".repeat(empty),
            cov.coverage_percent
        ));

        md.push_str("| Archivo | Cobertura | Lineas | Visual |\n");
        md.push_str("|---------|-----------|--------|--------|\n");

        let mut sorted: Vec<_> = cov.files.iter().collect();
        sorted.sort_by(|a, b| {
            a.coverage_percent
                .partial_cmp(&b.coverage_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for f in &sorted {
            let icon = if f.coverage_percent >= 80.0 {
                "🟢"
            } else if f.coverage_percent >= 50.0 {
                "🟡"
            } else {
                "🔴"
            };
            let bar_len = (f.coverage_percent / 10.0) as usize;
            let bar = "█".repeat(bar_len);
            let short_path = if f.path.len() > 50 {
                format!("...{}", &f.path[f.path.len() - 47..])
            } else {
                f.path.clone()
            };
            md.push_str(&format!(
                "| `{}` | {} {:.1}% | {}/{} | `{}` |\n",
                short_path, icon, f.coverage_percent, f.covered_lines, f.total_lines, bar
            ));
        }
        md.push('\n');
    }
}

fn write_explicit_debt(md: &mut String, result: &AnalysisResult) {
    if !result.explicit_debt.is_empty() {
        md.push_str("## Deuda Tecnica Explicita\n\n");

        let mut by_kind: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for item in &result.explicit_debt {
            *by_kind.entry(&item.kind).or_insert(0) += 1;
        }
        let summary: Vec<String> = by_kind
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        md.push_str(&format!(
            "**Total**: {} marcadores ({})\n\n",
            result.explicit_debt.len(),
            summary.join(", ")
        ));

        md.push_str("| Archivo | Linea | Tipo | Texto |\n");
        md.push_str("|---------|-------|------|-------|\n");
        for item in result.explicit_debt.iter().take(50) {
            let short = if item.file.len() > 40 {
                format!("...{}", &item.file[item.file.len() - 37..])
            } else {
                item.file.clone()
            };
            let text = if item.text.len() > 60 {
                format!("{}...", &item.text[..57])
            } else {
                item.text.clone()
            };
            md.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                short, item.line, item.kind, text
            ));
        }
        if result.explicit_debt.len() > 50 {
            md.push_str(&format!(
                "\n*... y {} mas*\n",
                result.explicit_debt.len() - 50
            ));
        }
        md.push('\n');
    }
}

/// Standard layer ordering for tables and diagrams.
pub(crate) const LAYERS_ORDER: [ArchLayer; 8] = [
    ArchLayer::Controller,
    ArchLayer::Service,
    ArchLayer::Repository,
    ArchLayer::Model,
    ArchLayer::Utility,
    ArchLayer::Config,
    ArchLayer::Test,
    ArchLayer::Unknown,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::complexity::{FileComplexity, FunctionComplexity};
    use crate::analyzer::coverage::{CoverageData, FileCoverage};
    use crate::analyzer::explicit_debt::ExplicitDebtItem;
    use crate::analyzer::patterns::antipatterns::{AntiPattern, AntiPatternKind, Severity};
    use crate::analyzer::patterns::{ArchAnalysis, ArchPattern};
    use std::collections::{HashMap, HashSet};

    fn make_graph(modules: Vec<ModuleNode>, edges: Vec<ImportEdge>) -> DependencyGraph {
        DependencyGraph {
            root_path: "/project".into(),
            primary_language: Language::Python,
            modules,
            edges,
            external_deps: HashSet::new(),
        }
    }

    fn make_module(path: &str, layer: ArchLayer, loc: usize, funcs: usize) -> ModuleNode {
        ModuleNode {
            path: path.to_string(),
            language: Language::Python,
            layer,
            loc,
            class_count: 1,
            function_count: funcs,
        }
    }

    fn make_analysis(graph: DependencyGraph) -> AnalysisResult {
        AnalysisResult {
            architecture: ArchAnalysis {
                detected_pattern: ArchPattern::Layered,
                confidence: 0.85,
                layer_distribution: {
                    let mut m = HashMap::new();
                    for module in &graph.modules {
                        *m.entry(module.layer).or_insert(0) += 1;
                    }
                    m
                },
                anti_patterns: vec![],
            },
            graph,
            coverage: None,
            complexity: None,
            explicit_debt: vec![],
        }
    }

    #[test]
    fn test_generate_docs_header() {
        let graph = make_graph(
            vec![make_module("svc.py", ArchLayer::Service, 100, 5)],
            vec![],
        );
        let result = make_analysis(graph);
        let md = generate_docs(&result, "TestProject");
        assert!(md.contains("# Arquitectura: TestProject"));
        assert!(md.contains("## Resumen"));
        assert!(md.contains("Layered"));
        assert!(md.contains("85%"));
    }

    #[test]
    fn test_generate_docs_layer_distribution() {
        let graph = make_graph(
            vec![
                make_module("ctrl.py", ArchLayer::Controller, 50, 3),
                make_module("svc.py", ArchLayer::Service, 100, 5),
            ],
            vec![],
        );
        let result = make_analysis(graph);
        let md = generate_docs(&result, "Test");
        assert!(md.contains("## Distribucion por Capas"));
        assert!(md.contains("Controller"));
        assert!(md.contains("Service"));
    }

    #[test]
    fn test_generate_docs_no_antipatterns() {
        let graph = make_graph(vec![make_module("a.py", ArchLayer::Service, 50, 3)], vec![]);
        let result = make_analysis(graph);
        let md = generate_docs(&result, "Clean");
        assert!(md.contains("No se detectaron anti-patrones significativos"));
    }

    #[test]
    fn test_generate_docs_with_antipatterns() {
        let graph = make_graph(
            vec![make_module("big.py", ArchLayer::Service, 600, 20)],
            vec![],
        );
        let mut result = make_analysis(graph);
        result.architecture.anti_patterns.push(AntiPattern {
            kind: AntiPatternKind::GodClass,
            description: "big.py es demasiado grande".into(),
            affected_modules: vec!["big.py".into()],
            severity: Severity::High,
            suggestion: "Dividir en modulos".into(),
        });
        result.architecture.anti_patterns.push(AntiPattern {
            kind: AntiPatternKind::ExcessiveCoupling,
            description: "coupling medio".into(),
            affected_modules: vec!["big.py".into()],
            severity: Severity::Medium,
            suggestion: "Reducir deps".into(),
        });
        let md = generate_docs(&result, "Bad");
        assert!(md.contains("## Anti-patrones Detectados"));
        assert!(md.contains("### Alta Severidad"));
        assert!(md.contains("God Class"));
        assert!(md.contains("### Severidad Media"));
    }

    #[test]
    fn test_generate_docs_mermaid_diagram() {
        let graph = make_graph(
            vec![
                make_module("ctrl.py", ArchLayer::Controller, 50, 3),
                make_module("svc.py", ArchLayer::Service, 100, 5),
            ],
            vec![ImportEdge {
                from: "ctrl.py".into(),
                to: "svc.py".into(),
                is_external: false,
            }],
        );
        let result = make_analysis(graph);
        let md = generate_docs(&result, "Test");
        assert!(md.contains("```mermaid"));
        assert!(md.contains("graph LR"));
        assert!(md.contains("ctrl_py --> svc_py"));
    }

    #[test]
    fn test_generate_docs_modules_table() {
        let graph = make_graph(
            vec![make_module("app.py", ArchLayer::Service, 200, 10)],
            vec![],
        );
        let result = make_analysis(graph);
        let md = generate_docs(&result, "Test");
        assert!(md.contains("## Modulos"));
        assert!(md.contains("`app.py`"));
    }

    #[test]
    fn test_generate_docs_external_deps() {
        let mut graph = make_graph(vec![make_module("a.py", ArchLayer::Service, 50, 3)], vec![]);
        graph.external_deps.insert("flask".into());
        graph.external_deps.insert("requests".into());
        let result = make_analysis(graph);
        let md = generate_docs(&result, "Test");
        assert!(md.contains("## Dependencias Externas"));
        assert!(md.contains("`flask`"));
        assert!(md.contains("`requests`"));
    }

    #[test]
    fn test_generate_docs_with_coverage() {
        let graph = make_graph(vec![make_module("a.py", ArchLayer::Service, 50, 3)], vec![]);
        let mut result = make_analysis(graph);
        result.coverage = Some(CoverageData {
            tool: "pytest-cov".into(),
            total_lines: 100,
            covered_lines: 75,
            coverage_percent: 75.0,
            files: vec![FileCoverage {
                path: "a.py".into(),
                total_lines: 100,
                covered_lines: 75,
                coverage_percent: 75.0,
            }],
        });
        let md = generate_docs(&result, "Test");
        assert!(md.contains("## Test Coverage"));
        assert!(md.contains("pytest-cov"));
        assert!(md.contains("75.0%"));
    }

    #[test]
    fn test_generate_docs_with_complexity() {
        let graph = make_graph(vec![make_module("a.py", ArchLayer::Service, 50, 3)], vec![]);
        let mut result = make_analysis(graph);
        result.complexity = Some(vec![(
            "a.py".into(),
            FileComplexity {
                functions: vec![FunctionComplexity {
                    name: "complex_fn".into(),
                    line: 10,
                    complexity: 12,
                    loc: 50,
                    has_coverage: None,
                }],
            },
        )]);
        let md = generate_docs(&result, "Test");
        assert!(md.contains("## Complejidad Ciclomatica"));
        assert!(md.contains("`complex_fn`"));
    }

    #[test]
    fn test_generate_docs_with_explicit_debt() {
        let graph = make_graph(vec![make_module("a.py", ArchLayer::Service, 50, 3)], vec![]);
        let mut result = make_analysis(graph);
        result.explicit_debt = vec![ExplicitDebtItem {
            file: "a.py".into(),
            line: 5,
            kind: "TODO".into(),
            text: "implement error handling".into(),
            language: "python".into(),
        }];
        let md = generate_docs(&result, "Test");
        assert!(md.contains("## Deuda Tecnica Explicita"));
        assert!(md.contains("TODO"));
        assert!(md.contains("implement error handling"));
    }

    #[test]
    fn test_generate_docs_footer() {
        let graph = make_graph(vec![make_module("a.py", ArchLayer::Service, 50, 3)], vec![]);
        let result = make_analysis(graph);
        let md = generate_docs(&result, "Test");
        assert!(md.contains("Generado automaticamente por VoidStack"));
    }

    #[test]
    fn test_generate_docs_coupling_metrics() {
        let graph = make_graph(
            vec![
                make_module("a.py", ArchLayer::Controller, 50, 3),
                make_module("b.py", ArchLayer::Service, 50, 3),
            ],
            vec![ImportEdge {
                from: "a.py".into(),
                to: "b.py".into(),
                is_external: false,
            }],
        );
        let result = make_analysis(graph);
        let md = generate_docs(&result, "Test");
        assert!(md.contains("## Metricas de Acoplamiento"));
    }

    #[test]
    fn test_modules_table_limited_to_30_sorted_by_loc() {
        let modules: Vec<ModuleNode> = (0..40)
            .map(|i| {
                make_module(
                    &format!("mod_{:02}.py", i),
                    ArchLayer::Service,
                    (i + 1) * 10,
                    2,
                )
            })
            .collect();
        let graph = make_graph(modules, vec![]);
        let result = make_analysis(graph);
        let md = generate_docs(&result, "Test");

        // Count table rows (lines starting with "| `")
        let table_rows = md.lines().filter(|l| l.starts_with("| `")).count();
        assert_eq!(table_rows, 30, "Should have exactly 30 module rows");

        // Verify overflow message
        assert!(md.contains("y 10 módulos más"));

        // Verify sorted by LOC desc: first row should be highest LOC (mod_39 = 400 LOC)
        let first_row = md.lines().find(|l| l.starts_with("| `")).unwrap();
        assert!(
            first_row.contains("mod_39.py"),
            "First row should be the module with highest LOC"
        );
    }
}
