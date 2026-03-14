//! Scoring-based architectural layer classifier.
//!
//! Classifies source code files into architectural layers (Controller, Service,
//! Repository, Model, Utility, Config, Test) using a weighted scoring system:
//!
//! 1. **Deterministic overrides** — test files, config files, entry points
//! 2. **Content signals** — patterns in the code (+1 to +5 weight each)
//! 3. **Directory bonus** — universal naming conventions (+2 bonus)
//! 4. **Fan-in/fan-out** — dependency graph position (post-scoring refinement)

mod signals;
#[cfg(test)]
mod tests;

use super::super::graph::*;
use signals::{CONTENT_SIGNALS, DIR_BONUS};

/// Classify a module into an architectural layer based on path and content.
pub(crate) fn classify_layer(path: &str, content: &str) -> ArchLayer {
    // 1. Deterministic path-level overrides (test, config, entry points)
    if let Some(layer) = classify_by_path_keywords(path) {
        return layer;
    }

    // 2. Score-based classification: content + path bonuses
    let scores = compute_layer_scores(path, content);

    // Find the layer with the highest score (ignore Unknown)
    let best = scores
        .iter()
        .filter(|(layer, _)| *layer != ArchLayer::Unknown)
        .max_by_key(|(_, score)| *score);

    match best {
        Some((layer, score)) if *score > 0 => *layer,
        _ => ArchLayer::Unknown,
    }
}

/// Refine Unknown modules using fan-in/fan-out from the dependency graph.
/// This is the dynamic, language-agnostic classification — no hardcoded names.
pub(crate) fn refine_unknown_by_graph(modules: &mut [ModuleNode], edges: &[ImportEdge]) {
    use std::collections::HashMap;

    let mut fan_in: HashMap<&str, usize> = HashMap::new();
    let mut fan_out: HashMap<&str, usize> = HashMap::new();

    for edge in edges {
        if !edge.is_external {
            *fan_in.entry(edge.to.as_str()).or_insert(0) += 1;
            *fan_out.entry(edge.from.as_str()).or_insert(0) += 1;
        }
    }

    // Calculate median fan-in to set adaptive thresholds
    let mut fan_in_values: Vec<usize> = modules
        .iter()
        .map(|m| *fan_in.get(m.path.as_str()).unwrap_or(&0))
        .filter(|v| *v > 0)
        .collect();
    fan_in_values.sort_unstable();
    let median_fan_in = fan_in_values
        .get(fan_in_values.len() / 2)
        .copied()
        .unwrap_or(1);
    let high_fan_in_threshold = (median_fan_in * 2).max(3);

    for module in modules.iter_mut() {
        if module.layer != ArchLayer::Unknown {
            continue;
        }

        let fi = *fan_in.get(module.path.as_str()).unwrap_or(&0);
        let fo = *fan_out.get(module.path.as_str()).unwrap_or(&0);

        if fi >= high_fan_in_threshold && fo <= 1 {
            module.layer = ArchLayer::Model;
        } else if fi >= high_fan_in_threshold {
            module.layer = ArchLayer::Utility;
        } else if fo >= high_fan_in_threshold && fi <= 1 {
            module.layer = ArchLayer::Controller;
        } else if fo > fi && fo >= 2 {
            module.layer = ArchLayer::Service;
        }
    }
}

// ── Deterministic overrides ────────────────────────────────────────────────

fn classify_by_path_keywords(path: &str) -> Option<ArchLayer> {
    let lower = path.to_lowercase();

    if lower.contains("test") || lower.contains("spec") || lower.starts_with("tests/") {
        return Some(ArchLayer::Test);
    }

    let config_suffixes = [
        "config.py",
        "config.js",
        "config.ts",
        "config.rs",
        "config.toml",
        "config.yaml",
        "config.yml",
    ];
    if lower.contains(".env") || config_suffixes.iter().any(|s| lower.ends_with(s)) {
        return Some(ArchLayer::Config);
    }

    if lower.ends_with("main.rs") || lower.ends_with("build.rs") || lower.ends_with("lib.rs") {
        return Some(ArchLayer::Utility);
    }

    None
}

// ── Scoring engine ─────────────────────────────────────────────────────────

/// Compute weighted scores for each architectural layer.
pub(crate) fn compute_layer_scores(path: &str, content: &str) -> Vec<(ArchLayer, i32)> {
    use std::collections::HashMap;
    let mut scores: HashMap<ArchLayer, i32> = HashMap::new();

    for signal in CONTENT_SIGNALS {
        if content.contains(signal.pattern) {
            *scores.entry(signal.layer).or_insert(0) += signal.weight;
        }
    }

    let parts: Vec<&str> = path.split('/').collect();
    for part in &parts {
        let p = part.to_lowercase();
        for (names, layer, bonus) in DIR_BONUS {
            if names.contains(&p.as_str()) {
                *scores.entry(*layer).or_insert(0) += bonus;
            }
        }
    }

    scores.into_iter().collect()
}
