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
    let label = if result.overall_score >= 90.0 {
        "Excelente"
    } else if result.overall_score >= 70.0 {
        "Bueno"
    } else if result.overall_score >= 50.0 {
        "Necesita trabajo"
    } else {
        "Crítico"
    };

    md.push_str(&format!(
        "**Overall Score: {:.0}/100** — {}\n",
        result.overall_score, label
    ));

    // Tools used
    let tools_desc: Vec<String> = result
        .tool_scores
        .iter()
        .map(|ts| {
            if let Some(ns) = ts.native_score {
                format!("{} (score: {:.0})", ts.tool, ns)
            } else {
                ts.tool.clone()
            }
        })
        .collect();
    md.push_str(&format!("*Tools used: {}*\n\n", tools_desc.join(", ")));

    if result.findings.is_empty() {
        md.push_str("✅ Best Practices Score: 100/100 — All checks passed across ");
        md.push_str(&format!("{} tools.\n\n", result.tools_used.len()));
        return md;
    }

    // Group by severity
    let important: Vec<_> = result
        .findings
        .iter()
        .filter(|f| f.severity == BpSeverity::Important)
        .collect();
    let warnings: Vec<_> = result
        .findings
        .iter()
        .filter(|f| f.severity == BpSeverity::Warning)
        .collect();
    let suggestions: Vec<_> = result
        .findings
        .iter()
        .filter(|f| f.severity == BpSeverity::Suggestion)
        .collect();

    if !important.is_empty() {
        md.push_str(&format!(
            "### 🔴 Important ({} findings)\n\n",
            important.len()
        ));
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
        md.push_str(&format!(
            "### 💡 Suggestion ({} findings)\n\n",
            suggestions.len()
        ));
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

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    fn make_finding(rule: &str, sev: BpSeverity, tool: &str) -> BestPracticesFinding {
        BestPracticesFinding {
            rule_id: rule.into(),
            tool: tool.into(),
            category: BpCategory::Correctness,
            severity: sev,
            file: "test.rs".into(),
            line: Some(10),
            col: None,
            message: format!("Finding {}", rule),
            fix_hint: None,
        }
    }

    #[test]
    fn test_no_tools_used() {
        let result = BestPracticesResult {
            findings: vec![],
            overall_score: 100.0,
            tool_scores: vec![],
            tools_used: vec![],
        };
        let md = generate_best_practices_markdown(&result);
        assert!(md.contains("No se encontraron herramientas de linting"));
    }

    #[test]
    fn test_perfect_score() {
        let result = BestPracticesResult {
            findings: vec![],
            overall_score: 100.0,
            tool_scores: vec![ToolScore {
                tool: "clippy".into(),
                score: 100.0,
                finding_count: 0,
                native_score: None,
            }],
            tools_used: vec!["clippy".into()],
        };
        let md = generate_best_practices_markdown(&result);
        assert!(md.contains("100/100"));
        assert!(md.contains("All checks passed"));
    }

    #[test]
    fn test_with_findings_grouped() {
        let result = BestPracticesResult {
            findings: vec![
                make_finding("E1", BpSeverity::Important, "ruff"),
                make_finding("W1", BpSeverity::Warning, "ruff"),
                make_finding("S1", BpSeverity::Suggestion, "ruff"),
            ],
            overall_score: 92.5,
            tool_scores: vec![ToolScore {
                tool: "ruff".into(),
                score: 92.5,
                finding_count: 3,
                native_score: None,
            }],
            tools_used: vec!["ruff".into()],
        };
        let md = generate_best_practices_markdown(&result);
        assert!(md.contains("Important (1 findings)"));
        assert!(md.contains("Warning (1 findings)"));
        assert!(md.contains("Suggestion (1 findings)"));
        assert!(md.contains("[E1]"));
        assert!(md.contains("[W1]"));
        assert!(md.contains("[S1]"));
    }

    #[test]
    fn test_score_label_excelente() {
        let result = BestPracticesResult {
            findings: vec![],
            overall_score: 95.0,
            tool_scores: vec![ToolScore {
                tool: "x".into(),
                score: 95.0,
                finding_count: 0,
                native_score: None,
            }],
            tools_used: vec!["x".into()],
        };
        let md = generate_best_practices_markdown(&result);
        assert!(md.contains("Excelente"));
    }

    #[test]
    fn test_score_label_bueno() {
        let result = BestPracticesResult {
            findings: vec![make_finding("R1", BpSeverity::Warning, "x")],
            overall_score: 75.0,
            tool_scores: vec![ToolScore {
                tool: "x".into(),
                score: 75.0,
                finding_count: 1,
                native_score: None,
            }],
            tools_used: vec!["x".into()],
        };
        let md = generate_best_practices_markdown(&result);
        assert!(md.contains("Bueno"));
    }

    #[test]
    fn test_native_score_footer() {
        let result = BestPracticesResult {
            findings: vec![make_finding("R1", BpSeverity::Suggestion, "react-doctor")],
            overall_score: 99.5,
            tool_scores: vec![ToolScore {
                tool: "react-doctor".into(),
                score: 99.5,
                finding_count: 1,
                native_score: Some(82.0),
            }],
            tools_used: vec!["react-doctor".into()],
        };
        let md = generate_best_practices_markdown(&result);
        assert!(md.contains("react-doctor Report: Score 82/100"));
    }

    #[test]
    fn test_finding_with_fix_hint() {
        let mut finding = make_finding("R1", BpSeverity::Warning, "ruff");
        finding.fix_hint = Some("Use `with` statement".into());
        let result = BestPracticesResult {
            findings: vec![finding],
            overall_score: 98.0,
            tool_scores: vec![],
            tools_used: vec!["ruff".into()],
        };
        let md = generate_best_practices_markdown(&result);
        assert!(md.contains("Fix: Use `with` statement"));
    }

    #[test]
    fn test_finding_with_file_no_line() {
        let mut finding = make_finding("R1", BpSeverity::Warning, "x");
        finding.line = None;
        let result = BestPracticesResult {
            findings: vec![finding],
            overall_score: 98.0,
            tool_scores: vec![],
            tools_used: vec!["x".into()],
        };
        let md = generate_best_practices_markdown(&result);
        assert!(md.contains("`test.rs`"));
        assert!(!md.contains(":10"));
    }
}
