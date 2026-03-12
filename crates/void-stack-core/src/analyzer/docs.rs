//! Markdown documentation generation from analysis results.

use std::collections::{HashMap, HashSet};

use super::graph::*;
use super::patterns::antipatterns::Severity;

/// Generate a coverage hint based on detected languages.
fn coverage_hint(graph: &DependencyGraph) -> Option<String> {
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
        hints.push("- **JS/TS**: `npx c8 --reporter=lcov npm test` (genera `coverage/lcov.info`)".into());
    }
    if languages.contains(&Language::Go) {
        hints.push("- **Go**: `go test -coverprofile=coverage.out ./...` (genera `coverage.out`)".into());
    }
    if languages.contains(&Language::Dart) {
        hints.push("- **Flutter**: `flutter test --coverage` (genera `coverage/lcov.info`)".into());
    }

    if hints.len() == 1 {
        return None; // No language-specific hints
    }

    Some(hints.join("\n"))
}

/// Generate a full markdown architecture document.
pub fn generate_docs(result: &super::AnalysisResult, project_name: &str) -> String {
    let mut md = String::new();

    // Header
    md.push_str(&format!("# Arquitectura: {}\n\n", project_name));

    // Overview
    md.push_str("## Resumen\n\n");
    md.push_str(&format!("| | |\n|---|---|\n"));
    md.push_str(&format!("| **Patron** | {} (confianza: {:.0}%) |\n", result.architecture.detected_pattern, result.architecture.confidence * 100.0));
    md.push_str(&format!("| **Lenguaje** | {} |\n", result.graph.primary_language));
    md.push_str(&format!("| **Modulos** | {} archivos |\n", result.graph.modules.len()));

    let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();
    md.push_str(&format!("| **LOC** | {} lineas |\n", total_loc));
    md.push_str(&format!("| **Deps externas** | {} paquetes |\n", result.graph.external_deps.len()));
    md.push_str("\n");

    // Layer distribution
    md.push_str("## Distribucion por Capas\n\n");
    md.push_str("| Capa | Archivos | LOC | % |\n");
    md.push_str("|------|----------|-----|---|\n");

    let mut layer_loc: HashMap<ArchLayer, usize> = HashMap::new();
    for m in &result.graph.modules {
        *layer_loc.entry(m.layer).or_insert(0) += m.loc;
    }

    let layers_order = [
        ArchLayer::Controller,
        ArchLayer::Service,
        ArchLayer::Repository,
        ArchLayer::Model,
        ArchLayer::Utility,
        ArchLayer::Config,
        ArchLayer::Test,
        ArchLayer::Unknown,
    ];

    for layer in &layers_order {
        let count = result.architecture.layer_distribution.get(layer).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let loc = layer_loc.get(layer).copied().unwrap_or(0);
        let pct = if total_loc > 0 { (loc as f32 / total_loc as f32 * 100.0) as u32 } else { 0 };
        md.push_str(&format!("| {} | {} | {} | {}% |\n", layer, count, loc, pct));
    }
    md.push_str("\n");

    // Anti-patterns
    if !result.architecture.anti_patterns.is_empty() {
        md.push_str("## Anti-patrones Detectados\n\n");

        let high: Vec<_> = result.architecture.anti_patterns.iter().filter(|a| a.severity == Severity::High).collect();
        let medium: Vec<_> = result.architecture.anti_patterns.iter().filter(|a| a.severity == Severity::Medium).collect();
        let low: Vec<_> = result.architecture.anti_patterns.iter().filter(|a| a.severity == Severity::Low).collect();

        if !high.is_empty() {
            md.push_str("### Alta Severidad\n\n");
            for ap in &high {
                md.push_str(&format!("- **{}**: {}\n", ap.kind, ap.description));
                md.push_str(&format!("  - *Sugerencia*: {}\n", ap.suggestion));
            }
            md.push_str("\n");
        }
        if !medium.is_empty() {
            md.push_str("### Severidad Media\n\n");
            for ap in &medium {
                md.push_str(&format!("- **{}**: {}\n", ap.kind, ap.description));
                md.push_str(&format!("  - *Sugerencia*: {}\n", ap.suggestion));
            }
            md.push_str("\n");
        }
        if !low.is_empty() {
            md.push_str("### Baja Severidad\n\n");
            for ap in &low {
                md.push_str(&format!("- **{}**: {}\n", ap.kind, ap.description));
                md.push_str(&format!("  - *Sugerencia*: {}\n", ap.suggestion));
            }
            md.push_str("\n");
        }
    } else {
        md.push_str("## Anti-patrones\n\nNo se detectaron anti-patrones significativos.\n\n");
    }

    // Dependency map (Mermaid)
    md.push_str("## Mapa de Dependencias\n\n");
    md.push_str("```mermaid\ngraph LR\n");

    // Group modules by layer for visual clarity
    for layer in &layers_order {
        let modules_in_layer: Vec<_> = result.graph.modules.iter()
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

    // Internal edges only
    let internal_edges: Vec<_> = result.graph.edges.iter()
        .filter(|e| !e.is_external)
        .filter(|e| result.graph.modules.iter().any(|m| m.path == e.to))
        .collect();

    // Limit edges to avoid overwhelming diagrams
    let max_edges = 50;
    for edge in internal_edges.iter().take(max_edges) {
        let from_id = sanitize_id(&edge.from);
        let to_id = sanitize_id(&edge.to);
        md.push_str(&format!("    {} --> {}\n", from_id, to_id));
    }
    if internal_edges.len() > max_edges {
        md.push_str(&format!("    %% ... y {} conexiones mas\n", internal_edges.len() - max_edges));
    }

    md.push_str("```\n\n");

    // Module details
    md.push_str("## Modulos\n\n");
    md.push_str("| Archivo | Capa | LOC | Clases | Funciones |\n");
    md.push_str("|---------|------|-----|--------|----------|\n");

    let mut sorted_modules: Vec<_> = result.graph.modules.iter().collect();
    sorted_modules.sort_by(|a, b| a.path.cmp(&b.path));

    for m in &sorted_modules {
        md.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            m.path, m.layer, m.loc, m.class_count, m.function_count
        ));
    }
    md.push_str("\n");

    // External dependencies
    if !result.graph.external_deps.is_empty() {
        md.push_str("## Dependencias Externas\n\n");
        let mut deps: Vec<_> = result.graph.external_deps.iter().collect();
        deps.sort();
        for dep in &deps {
            md.push_str(&format!("- `{}`\n", dep));
        }
        md.push_str("\n");
    }

    // Cyclomatic Complexity
    if let Some(cx_data) = &result.complexity {
        md.push_str("## Complejidad Ciclomatica\n\n");

        let all_funcs: Vec<_> = cx_data.iter()
            .flat_map(|(path, fc)| fc.functions.iter().map(move |f| (path.as_str(), f)))
            .collect();

        if !all_funcs.is_empty() {
            let total_cx: usize = all_funcs.iter().map(|(_, f)| f.complexity).sum();
            let avg_cx = total_cx as f32 / all_funcs.len() as f32;
            let _max_func = all_funcs.iter().max_by_key(|(_, f)| f.complexity);
            let complex_count = all_funcs.iter().filter(|(_, f)| f.complexity >= 10).count();

            md.push_str(&format!("**Promedio**: {:.1} | **Funciones analizadas**: {} | **Funciones complejas (>=10)**: {}\n\n",
                avg_cx, all_funcs.len(), complex_count));

            // Top complex functions
            let mut sorted: Vec<_> = all_funcs.iter()
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
                    let icon = if func.complexity >= 15 { "!!" }
                        else if func.complexity >= 10 { "!" }
                        else { "" };
                    let short = path.rsplit('/').next().unwrap_or(path);
                    if has_any_coverage {
                        let cov_icon = match func.has_coverage {
                            Some(true) => "✅",
                            Some(false) => "🔴",
                            None => "-",
                        };
                        md.push_str(&format!("| `{}` {} | `{}` | {} | {} | {} | {} |\n",
                            func.name, icon, short, func.line, func.complexity, func.loc, cov_icon));
                    } else {
                        md.push_str(&format!("| `{}` {} | `{}` | {} | {} | {} |\n",
                            func.name, icon, short, func.line, func.complexity, func.loc));
                    }
                }
                md.push_str("\n");
            }
        }
    }

    // Coupling metrics
    md.push_str("## Metricas de Acoplamiento\n\n");
    md.push_str("| Modulo | Fan-in | Fan-out |\n");
    md.push_str("|--------|--------|--------|\n");

    let metrics = result.graph.coupling_metrics();
    let mut metric_list: Vec<_> = metrics.iter().collect();
    metric_list.sort_by(|a, b| b.1.1.cmp(&a.1.1)); // Sort by fan-out desc

    for (path, (fan_in, fan_out)) in metric_list.iter().take(20) {
        if *fan_in + *fan_out == 0 {
            continue;
        }
        let short = path.rsplit('/').next().unwrap_or(path);
        md.push_str(&format!("| `{}` | {} | {} |\n", short, fan_in, fan_out));
    }
    md.push_str("\n");

    // Test Coverage
    if result.coverage.is_none() {
        // Show hint about how to generate coverage
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
        md.push_str(&format!("**Herramienta**: {} | **Cobertura total**: {:.1}% ({}/{} lineas)\n\n",
            cov.tool, cov.coverage_percent, cov.covered_lines, cov.total_lines));

        // Visual bar
        let filled = (cov.coverage_percent / 5.0) as usize;
        let empty = 20_usize.saturating_sub(filled);
        let bar_color = if cov.coverage_percent >= 80.0 { "🟢" }
            else if cov.coverage_percent >= 50.0 { "🟡" }
            else { "🔴" };
        md.push_str(&format!("{} `[{}{}]` {:.1}%\n\n",
            bar_color,
            "█".repeat(filled),
            "░".repeat(empty),
            cov.coverage_percent));

        // Per-file table sorted by coverage (worst first)
        md.push_str("| Archivo | Cobertura | Lineas | Visual |\n");
        md.push_str("|---------|-----------|--------|--------|\n");

        let mut sorted: Vec<_> = cov.files.iter().collect();
        sorted.sort_by(|a, b| a.coverage_percent.partial_cmp(&b.coverage_percent).unwrap_or(std::cmp::Ordering::Equal));

        for f in &sorted {
            let icon = if f.coverage_percent >= 80.0 { "🟢" }
                else if f.coverage_percent >= 50.0 { "🟡" }
                else { "🔴" };
            let bar_len = (f.coverage_percent / 10.0) as usize;
            let bar = "█".repeat(bar_len);
            let short_path = if f.path.len() > 50 {
                format!("...{}", &f.path[f.path.len()-47..])
            } else {
                f.path.clone()
            };
            md.push_str(&format!(
                "| `{}` | {} {:.1}% | {}/{} | `{}` |\n",
                short_path, icon, f.coverage_percent,
                f.covered_lines, f.total_lines, bar
            ));
        }
        md.push_str("\n");
    }

    // Explicit Debt (TODO/FIXME/HACK)
    if !result.explicit_debt.is_empty() {
        md.push_str("## Deuda Tecnica Explicita\n\n");

        let mut by_kind: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for item in &result.explicit_debt {
            *by_kind.entry(&item.kind).or_insert(0) += 1;
        }
        let summary: Vec<String> = by_kind.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
        md.push_str(&format!("**Total**: {} marcadores ({})\n\n", result.explicit_debt.len(), summary.join(", ")));

        md.push_str("| Archivo | Linea | Tipo | Texto |\n");
        md.push_str("|---------|-------|------|-------|\n");
        for item in result.explicit_debt.iter().take(50) {
            let short = if item.file.len() > 40 {
                format!("...{}", &item.file[item.file.len()-37..])
            } else {
                item.file.clone()
            };
            let text = if item.text.len() > 60 {
                format!("{}...", &item.text[..57])
            } else {
                item.text.clone()
            };
            md.push_str(&format!("| `{}` | {} | {} | {} |\n", short, item.line, item.kind, text));
        }
        if result.explicit_debt.len() > 50 {
            md.push_str(&format!("\n*... y {} mas*\n", result.explicit_debt.len() - 50));
        }
        md.push_str("\n");
    }

    md.push_str("---\n*Generado automaticamente por VoidStack*\n");

    md
}

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
