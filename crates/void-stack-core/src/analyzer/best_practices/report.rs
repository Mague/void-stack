//! Markdown report generation for best practices analysis.

use super::{BestPracticesResult, BpSeverity};

/// Generate a markdown section for best practices findings.
pub fn generate_best_practices_markdown(result: &BestPracticesResult) -> String {
    let mut md = String::new();

    md.push_str("## Best Practices\n\n");

    if result.tools_used.is_empty() {
        md.push_str("No se encontraron herramientas de linting aplicables.\n\n");
        return md;
    }

    // Overall score
    let label = if result.overall_score >= 90.0 { "Excelente" }
        else if result.overall_score >= 70.0 { "Bueno" }
        else if result.overall_score >= 50.0 { "Necesita trabajo" }
        else { "Crítico" };

    md.push_str(&format!("**Overall Score: {:.0}/100** — {}\n", result.overall_score, label));

    // Tools used
    let tools_desc: Vec<String> = result.tool_scores.iter().map(|ts| {
        if let Some(ns) = ts.native_score {
            format!("{} (score: {:.0})", ts.tool, ns)
        } else {
            ts.tool.clone()
        }
    }).collect();
    md.push_str(&format!("*Tools used: {}*\n\n", tools_desc.join(", ")));

    if result.findings.is_empty() {
        md.push_str("✅ Best Practices Score: 100/100 — All checks passed across ");
        md.push_str(&format!("{} tools.\n\n", result.tools_used.len()));
        return md;
    }

    // Group by severity
    let important: Vec<_> = result.findings.iter().filter(|f| f.severity == BpSeverity::Important).collect();
    let warnings: Vec<_> = result.findings.iter().filter(|f| f.severity == BpSeverity::Warning).collect();
    let suggestions: Vec<_> = result.findings.iter().filter(|f| f.severity == BpSeverity::Suggestion).collect();

    if !important.is_empty() {
        md.push_str(&format!("### 🔴 Important ({} findings)\n\n", important.len()));
        for f in &important {
            write_finding(&mut md, f);
        }
        md.push('\n');
    }

    if !warnings.is_empty() {
        md.push_str(&format!("### ⚠️ Warning ({} findings)\n\n", warnings.len()));
        for f in &warnings {
            write_finding(&mut md, f);
        }
        md.push('\n');
    }

    if !suggestions.is_empty() {
        md.push_str(&format!("### 💡 Suggestion ({} findings)\n\n", suggestions.len()));
        for f in &suggestions {
            write_finding(&mut md, f);
        }
        md.push('\n');
    }

    // Tool-specific footer
    for ts in &result.tool_scores {
        if let Some(ns) = ts.native_score {
            md.push_str(&format!("*{} Report: Score {:.0}/100*\n", ts.tool, ns));
        }
    }

    md.push_str("*Run `void analyze <project> --best-practices` to refresh*\n\n");
    md
}

fn write_finding(md: &mut String, f: &super::BestPracticesFinding) {
    let loc = if !f.file.is_empty() {
        if let Some(line) = f.line {
            format!(" `{}:{}`", f.file, line)
        } else {
            format!(" `{}`", f.file)
        }
    } else {
        String::new()
    };

    md.push_str(&format!("- **[{}]**{} — {}\n", f.rule_id, loc, f.message));
    if let Some(ref hint) = f.fix_hint {
        md.push_str(&format!("  > Fix: {}\n", hint));
    }
}
