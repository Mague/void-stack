//! Enrichment pipeline: adds syntactic context to each finding and adjusts
//! severity based on multi-language heuristics.

use std::collections::HashMap;
use std::path::Path;

use super::context::*;
use super::findings::*;

/// Callers threshold above which a function is considered a "hot path".
/// Reserved for future use when structural graph integration is wired in.
#[allow(dead_code)]
const HOT_PATH_THRESHOLD: usize = 10;

/// Enrich every finding with context + adjusted severity.
pub fn enrich_findings(
    findings: Vec<SecurityFinding>,
    project_root: &Path,
) -> Vec<SecurityFinding> {
    let mut file_cache: HashMap<String, String> = HashMap::new();

    findings
        .into_iter()
        .map(|mut f| {
            let fp = f.file_path.clone().unwrap_or_default();

            // Read file once (cached)
            let content = file_cache
                .entry(fp.clone())
                .or_insert_with(|| {
                    std::fs::read_to_string(project_root.join(&fp)).unwrap_or_default()
                })
                .clone();

            let role = detect_module_role(&fp);
            let language = detect_language(&fp).to_string();
            let in_const = f
                .line_number
                .map(|l| detect_const_context(&content, l as usize, &fp))
                .unwrap_or(false);
            let in_test = matches!(role, ModuleRole::Test);

            f.context = FindingContext {
                in_test_file: in_test,
                in_const_context: in_const,
                in_hot_path: false,
                callers_count: 0,
                module_role: role,
                language,
                surrounding_lines: f
                    .line_number
                    .map(|l| surrounding_lines(&content, l as usize, 3))
                    .unwrap_or_default(),
            };

            let (adj, conf, reason) = adjust_severity(&f);
            f.adjusted_severity = Some(adj);
            f.confidence = conf;
            f.adjustment_reason = Some(reason.to_string());
            f
        })
        .collect()
}

/// Apply severity adjustment rules. First match wins.
fn adjust_severity(f: &SecurityFinding) -> (Severity, Confidence, &'static str) {
    // 1. Unsafe error handling in const/static init — compile-time safe
    if matches!(f.category, FindingCategory::UnsafeErrorHandling) && f.context.in_const_context {
        return (
            Severity::Info,
            Confidence::Certain,
            "Static init, compile-time safe",
        );
    }
    // 2. Unsafe error handling in test — panics acceptable
    if matches!(f.category, FindingCategory::UnsafeErrorHandling) && f.context.in_test_file {
        return (
            Severity::Info,
            Confidence::Certain,
            "Test code, panics acceptable",
        );
    }
    // 3. Hardcoded secret in test — likely fixture
    if matches!(f.category, FindingCategory::HardcodedSecret) && f.context.in_test_file {
        return (
            Severity::Low,
            Confidence::Probable,
            "Likely fixture — verify manually",
        );
    }
    // 4. CC in i18n — translation table, not logic
    if matches!(
        f.category,
        FindingCategory::WeakCrypto | FindingCategory::WeakCryptography
    ) && matches!(f.context.module_role, ModuleRole::I18n)
    {
        return (
            Severity::Info,
            Confidence::Certain,
            "Translation table, not logic",
        );
    }
    // 5. Generated code — downgrade everything
    if matches!(f.context.module_role, ModuleRole::Generated) {
        return (Severity::Info, Confidence::Certain, "Auto-generated code");
    }
    // 6. Hot path + unsafe error handling (non-test) — upgrade
    if matches!(f.category, FindingCategory::UnsafeErrorHandling)
        && f.context.in_hot_path
        && !f.context.in_test_file
    {
        return (
            Severity::High,
            Confidence::Probable,
            "Hot path, wide blast radius",
        );
    }
    // 7. CVE — keep as-is with Certain confidence
    if matches!(f.category, FindingCategory::DependencyVulnerability) {
        return (
            f.severity,
            Confidence::Certain,
            "From CVE advisory database",
        );
    }
    // 8. Unsafe error handling in audit module itself — expected pattern
    if matches!(f.category, FindingCategory::UnsafeErrorHandling)
        && matches!(f.context.module_role, ModuleRole::Audit)
    {
        return (
            Severity::Info,
            Confidence::Probable,
            "Audit module — detection code, not production path",
        );
    }
    // Default: retain base severity
    (f.severity, Confidence::Heuristic, "Default retention")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_finding_with<F: FnOnce(&mut SecurityFinding)>(
        category: FindingCategory,
        configure: F,
    ) -> SecurityFinding {
        let mut f = SecurityFinding {
            id: "test".into(),
            severity: Severity::Medium,
            category,
            title: "t".into(),
            description: "d".into(),
            file_path: Some("src/lib.rs".into()),
            line_number: Some(1),
            remediation: "r".into(),
            adjusted_severity: None,
            confidence: Confidence::Heuristic,
            adjustment_reason: None,
            context: FindingContext::default(),
        };
        configure(&mut f);
        f
    }

    #[test]
    fn test_unwrap_in_const_downgraded_to_info() {
        let f = make_finding_with(FindingCategory::UnsafeErrorHandling, |f| {
            f.context.in_const_context = true;
        });
        let (sev, conf, _) = adjust_severity(&f);
        assert_eq!(sev, Severity::Info);
        assert_eq!(conf, Confidence::Certain);
    }

    #[test]
    fn test_unwrap_in_test_downgraded_to_info() {
        let f = make_finding_with(FindingCategory::UnsafeErrorHandling, |f| {
            f.context.in_test_file = true;
        });
        let (sev, _, _) = adjust_severity(&f);
        assert_eq!(sev, Severity::Info);
    }

    #[test]
    fn test_secret_in_test_downgraded_to_low() {
        let f = make_finding_with(FindingCategory::HardcodedSecret, |f| {
            f.context.in_test_file = true;
        });
        let (sev, conf, _) = adjust_severity(&f);
        assert_eq!(sev, Severity::Low);
        assert_eq!(conf, Confidence::Probable);
    }

    #[test]
    fn test_generated_code_downgraded() {
        let f = make_finding_with(FindingCategory::UnsafeErrorHandling, |f| {
            f.context.module_role = ModuleRole::Generated;
        });
        let (sev, _, _) = adjust_severity(&f);
        assert_eq!(sev, Severity::Info);
    }

    #[test]
    fn test_hot_path_upgraded_to_high() {
        let f = make_finding_with(FindingCategory::UnsafeErrorHandling, |f| {
            f.context.in_hot_path = true;
            f.context.callers_count = 15;
        });
        let (sev, _, _) = adjust_severity(&f);
        assert_eq!(sev, Severity::High);
    }

    #[test]
    fn test_cve_stays_certain() {
        let f = make_finding_with(FindingCategory::DependencyVulnerability, |_| {});
        let (_, conf, _) = adjust_severity(&f);
        assert_eq!(conf, Confidence::Certain);
    }

    #[test]
    fn test_audit_module_downgraded() {
        let f = make_finding_with(FindingCategory::UnsafeErrorHandling, |f| {
            f.context.module_role = ModuleRole::Audit;
        });
        let (sev, _, _) = adjust_severity(&f);
        assert_eq!(sev, Severity::Info);
    }

    #[test]
    fn test_default_retention() {
        let f = make_finding_with(FindingCategory::SqlInjection, |_| {});
        let (sev, conf, _) = adjust_severity(&f);
        assert_eq!(sev, Severity::Medium);
        assert_eq!(conf, Confidence::Heuristic);
    }
}
