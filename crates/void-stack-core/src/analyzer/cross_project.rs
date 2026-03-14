//! Cross-project coupling detection.
//!
//! Matches external dependencies found during analysis against
//! other registered VoidStack projects to discover inter-project relationships.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A detected relationship between two projects.
#[derive(Debug, Clone)]
pub struct ProjectLink {
    /// Project that imports/depends on the other.
    pub from_project: String,
    /// Service within from_project where the dependency was found.
    pub from_service: String,
    /// The other registered project being depended upon.
    pub to_project: String,
    /// The external dep name that matched.
    pub via_dependency: String,
}

/// Result of cross-project analysis.
#[derive(Debug, Clone)]
pub struct CrossProjectResult {
    pub links: Vec<ProjectLink>,
    /// External deps that did NOT match any registered project (truly external).
    pub unmatched_external: HashSet<String>,
}

/// Detect inter-project dependencies.
///
/// Takes a map of (project_name -> (service_name -> external_deps)) and
/// a map of (project_name -> known_package_names) where known_package_names
/// are names by which the project might be imported.
pub fn detect_cross_project(
    project_deps: &HashMap<String, Vec<(String, HashSet<String>)>>,
    project_identifiers: &HashMap<String, Vec<String>>,
) -> CrossProjectResult {
    let mut links = Vec::new();
    let mut all_matched = HashSet::new();

    for (from_proj, services) in project_deps {
        for (from_svc, ext_deps) in services {
            for dep in ext_deps {
                // Check if this external dep matches any other registered project
                for (to_proj, identifiers) in project_identifiers {
                    if to_proj == from_proj {
                        continue; // Don't match against self
                    }
                    if identifiers.iter().any(|id| matches_dep(dep, id)) {
                        links.push(ProjectLink {
                            from_project: from_proj.clone(),
                            from_service: from_svc.clone(),
                            to_project: to_proj.clone(),
                            via_dependency: dep.clone(),
                        });
                        all_matched.insert(dep.clone());
                    }
                }
            }
        }
    }

    // Collect truly external deps
    let all_external: HashSet<String> = project_deps
        .values()
        .flat_map(|services| services.iter().flat_map(|(_, deps)| deps.iter().cloned()))
        .collect();
    let unmatched = all_external.difference(&all_matched).cloned().collect();

    CrossProjectResult {
        links,
        unmatched_external: unmatched,
    }
}

/// Build project identifiers from project metadata.
///
/// A project can be identified by:
/// - Its VoidStack name (case-insensitive)
/// - Its directory name
/// - Package names from manifest files (package.json name, setup.py name, Cargo.toml name)
pub fn build_identifiers(projects: &[crate::model::Project]) -> HashMap<String, Vec<String>> {
    let mut result = HashMap::new();

    for project in projects {
        let mut ids = Vec::new();

        // Project name (normalized)
        ids.push(project.name.to_lowercase());
        ids.push(project.name.replace('-', "_").to_lowercase());
        ids.push(project.name.replace('_', "-").to_lowercase());

        // Directory name
        let path = crate::runner::local::strip_win_prefix(&project.path);
        if let Some(dir_name) = Path::new(&path).file_name().and_then(|n| n.to_str()) {
            let lower = dir_name.to_lowercase();
            if !ids.contains(&lower) {
                ids.push(lower.clone());
                ids.push(lower.replace('-', "_"));
                ids.push(lower.replace('_', "-"));
            }
        }

        // Scan for package names in manifests
        let manifest_names = scan_package_names(Path::new(&path));
        for name in manifest_names {
            let lower = name.to_lowercase();
            if !ids.contains(&lower) {
                ids.push(lower);
            }
        }

        // Also scan service directories
        for svc in &project.services {
            if let Some(wd) = &svc.working_dir {
                let clean = crate::runner::local::strip_win_prefix(wd);
                let svc_names = scan_package_names(Path::new(&clean));
                for name in svc_names {
                    let lower = name.to_lowercase();
                    if !ids.contains(&lower) {
                        ids.push(lower);
                    }
                }
            }
        }

        result.insert(project.name.clone(), ids);
    }

    result
}

/// Scan a directory for package/module names from manifest files.
fn scan_package_names(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();

    // package.json -> "name" field
    let pkg_json = dir.join("package.json");
    if let Ok(content) = std::fs::read_to_string(&pkg_json)
        && let Some(name) = extract_json_string(&content, "name")
    {
        // Strip scope prefix: @org/name -> name
        let clean = name.rsplit('/').next().unwrap_or(&name);
        names.push(clean.to_string());
    }

    // pyproject.toml -> [project] name
    let pyproject = dir.join("pyproject.toml");
    if let Ok(content) = std::fs::read_to_string(&pyproject)
        && let Some(name) = extract_toml_name(&content)
    {
        names.push(name);
    }

    // setup.py -> name= in setup()
    let setup_py = dir.join("setup.py");
    if let Ok(content) = std::fs::read_to_string(&setup_py)
        && let Some(name) = extract_setup_py_name(&content)
    {
        names.push(name);
    }

    // Cargo.toml -> [package] name
    let cargo_toml = dir.join("Cargo.toml");
    if let Ok(content) = std::fs::read_to_string(&cargo_toml)
        && let Some(name) = extract_toml_name(&content)
    {
        names.push(name);
    }

    // Go module name from go.mod
    let go_mod = dir.join("go.mod");
    if let Ok(content) = std::fs::read_to_string(&go_mod) {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("module ") {
                let module = rest.trim();
                // Last segment: github.com/user/pkg -> pkg
                if let Some(last) = module.rsplit('/').next() {
                    names.push(last.to_string());
                }
                names.push(module.to_string());
                break;
            }
        }
    }

    names
}

/// Check if an external dependency name matches a project identifier.
fn matches_dep(dep: &str, identifier: &str) -> bool {
    let dep_lower = dep.to_lowercase();
    let id_lower = identifier.to_lowercase();

    if dep_lower == id_lower {
        return true;
    }

    // Check with common transformations
    let dep_normalized = dep_lower.replace('-', "_");
    let id_normalized = id_lower.replace('-', "_");
    if dep_normalized == id_normalized {
        return true;
    }

    // Check if dep contains the identifier as a significant part
    // e.g., "humboldt_client" matches project "humbolt_reader" if identifier is "humboldt"
    // But be conservative: only exact matches or prefix matches
    if dep_normalized.starts_with(&format!("{}_", id_normalized)) {
        return true;
    }
    if dep_normalized.starts_with(&format!("{}-", id_lower)) {
        return true;
    }

    false
}

/// Simple JSON string extraction (no full parser needed).
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let pos = json.find(&pattern)?;
    let after = &json[pos + pattern.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim();

    let quote = rest.chars().next()?;
    if quote != '"' {
        return None;
    }
    let inner = &rest[1..];
    let end = inner.find('"')?;
    Some(inner[..end].to_string())
}

/// Extract "name" from TOML content (simple regex-free approach).
fn extract_toml_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name")
            && let Some(eq_pos) = trimmed.find('=')
        {
            let value = trimmed[eq_pos + 1..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Extract name from setup.py setup() call.
fn extract_setup_py_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name=") || trimmed.starts_with("name =") {
            let eq_pos = trimmed.find('=')?;
            let value = trimmed[eq_pos + 1..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .trim_end_matches(',');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Generate Mermaid diagram of project relationships.
pub fn links_to_mermaid(links: &[ProjectLink]) -> String {
    if links.is_empty() {
        return String::new();
    }

    let mut md = String::from("```mermaid\ngraph LR\n");

    // Deduplicate: one arrow per (from_project -> to_project)
    let mut seen = HashSet::new();
    for link in links {
        let key = format!("{}->{}", link.from_project, link.to_project);
        if seen.insert(key) {
            let from_id = sanitize_id(&link.from_project);
            let to_id = sanitize_id(&link.to_project);
            md.push_str(&format!(
                "    {}[\"{}\"] -->|{}| {}[\"{}\"]  \n",
                from_id, link.from_project, link.via_dependency, to_id, link.to_project,
            ));
        }
    }

    md.push_str("```\n");
    md
}

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Generate markdown report of cross-project dependencies.
pub fn cross_project_markdown(result: &CrossProjectResult) -> String {
    let mut md = String::new();

    md.push_str("## Acoplamiento Inter-Proyecto\n\n");

    if result.links.is_empty() {
        md.push_str("No se detectaron dependencias entre proyectos registrados.\n\n");
        return md;
    }

    md.push_str("| Desde | Servicio | Hacia | Via |\n");
    md.push_str("|-------|----------|-------|-----|\n");
    for link in &result.links {
        md.push_str(&format!(
            "| {} | {} | {} | `{}` |\n",
            link.from_project, link.from_service, link.to_project, link.via_dependency
        ));
    }
    md.push('\n');

    md.push_str(&links_to_mermaid(&result.links));
    md.push('\n');

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_string() {
        let json = r#"{"name": "my-package", "version": "1.0.0"}"#;
        assert_eq!(
            extract_json_string(json, "name"),
            Some("my-package".to_string())
        );
        assert_eq!(
            extract_json_string(json, "version"),
            Some("1.0.0".to_string())
        );
        assert_eq!(extract_json_string(json, "missing"), None);
    }

    #[test]
    fn test_extract_json_string_scoped() {
        let json = r#"{"name": "@org/pkg"}"#;
        assert_eq!(
            extract_json_string(json, "name"),
            Some("@org/pkg".to_string())
        );
    }

    #[test]
    fn test_extract_toml_name() {
        let toml = "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"";
        assert_eq!(extract_toml_name(toml), Some("my-crate".to_string()));
    }

    #[test]
    fn test_extract_toml_name_single_quotes() {
        let toml = "name = 'my-pkg'";
        assert_eq!(extract_toml_name(toml), Some("my-pkg".to_string()));
    }

    #[test]
    fn test_extract_toml_name_missing() {
        assert_eq!(extract_toml_name("version = \"1.0\""), None);
    }

    #[test]
    fn test_extract_setup_py_name() {
        // No trailing comma: trim_matches strips quotes cleanly
        let content = "from setuptools import setup\nsetup(\n    name='my-lib'\n)";
        assert_eq!(extract_setup_py_name(content), Some("my-lib".to_string()));
    }

    #[test]
    fn test_extract_setup_py_name_double_quotes() {
        let content = "    name = \"my_pkg\"";
        assert_eq!(extract_setup_py_name(content), Some("my_pkg".to_string()));
    }

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("my-project"), "my_project");
        assert_eq!(sanitize_id("hello_world"), "hello_world");
        assert_eq!(sanitize_id("a.b/c"), "a_b_c");
    }

    #[test]
    fn test_links_to_mermaid_empty() {
        assert_eq!(links_to_mermaid(&[]), "");
    }

    #[test]
    fn test_links_to_mermaid_with_links() {
        let links = vec![ProjectLink {
            from_project: "proj-a".into(),
            from_service: "svc".into(),
            to_project: "proj-b".into(),
            via_dependency: "proj_b_client".into(),
        }];
        let md = links_to_mermaid(&links);
        assert!(md.contains("```mermaid"));
        assert!(md.contains("graph LR"));
        assert!(md.contains("proj_a"));
        assert!(md.contains("proj_b"));
    }

    #[test]
    fn test_links_to_mermaid_deduplicates() {
        let links = vec![
            ProjectLink {
                from_project: "a".into(),
                from_service: "s1".into(),
                to_project: "b".into(),
                via_dependency: "x".into(),
            },
            ProjectLink {
                from_project: "a".into(),
                from_service: "s2".into(),
                to_project: "b".into(),
                via_dependency: "y".into(),
            },
        ];
        let md = links_to_mermaid(&links);
        // Only one arrow a->b
        assert_eq!(md.matches("-->").count(), 1);
    }

    #[test]
    fn test_cross_project_markdown_empty() {
        let result = CrossProjectResult {
            links: vec![],
            unmatched_external: HashSet::new(),
        };
        let md = cross_project_markdown(&result);
        assert!(md.contains("No se detectaron dependencias"));
    }

    #[test]
    fn test_cross_project_markdown_with_links() {
        let result = CrossProjectResult {
            links: vec![ProjectLink {
                from_project: "alpha".into(),
                from_service: "api".into(),
                to_project: "beta".into(),
                via_dependency: "beta_client".into(),
            }],
            unmatched_external: HashSet::new(),
        };
        let md = cross_project_markdown(&result);
        assert!(md.contains("| alpha | api | beta | `beta_client` |"));
    }

    #[test]
    fn test_scan_package_names_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "@scope/my-pkg"}"#,
        )
        .unwrap();
        let names = scan_package_names(dir.path());
        assert!(names.contains(&"my-pkg".to_string()));
    }

    #[test]
    fn test_scan_package_names_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\n",
        )
        .unwrap();
        let names = scan_package_names(dir.path());
        assert!(names.contains(&"my-crate".to_string()));
    }

    #[test]
    fn test_scan_package_names_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module github.com/user/mymod\n").unwrap();
        let names = scan_package_names(dir.path());
        assert!(names.contains(&"mymod".to_string()));
        assert!(names.contains(&"github.com/user/mymod".to_string()));
    }

    #[test]
    fn test_detect_cross_project_no_match() {
        let mut deps = HashMap::new();
        deps.insert(
            "proj_a".to_string(),
            vec![("svc".to_string(), {
                let mut s = HashSet::new();
                s.insert("react".to_string());
                s
            })],
        );
        let mut ids = HashMap::new();
        ids.insert("proj_a".to_string(), vec!["proj_a".to_string()]);
        let result = detect_cross_project(&deps, &ids);
        assert!(result.links.is_empty());
        assert!(result.unmatched_external.contains("react"));
    }

    #[test]
    fn test_matches_dep() {
        assert!(matches_dep("humboldt", "humboldt"));
        assert!(matches_dep("humboldt_client", "humboldt"));
        assert!(matches_dep("my-lib", "my_lib"));
        assert!(!matches_dep("react", "humboldt"));
        assert!(!matches_dep("fastapi", "fast"));
    }

    #[test]
    fn test_detect_cross_project() {
        let mut deps = HashMap::new();
        deps.insert(
            "san_luis".to_string(),
            vec![("gui-backend".to_string(), {
                let mut s = HashSet::new();
                s.insert("humboldt_client".to_string());
                s.insert("fastapi".to_string());
                s
            })],
        );
        deps.insert(
            "humboldt".to_string(),
            vec![("api".to_string(), {
                let mut s = HashSet::new();
                s.insert("flask".to_string());
                s
            })],
        );

        let mut identifiers = HashMap::new();
        identifiers.insert(
            "san_luis".to_string(),
            vec![
                "san_luis".to_string(),
                "san_luis_terrain_project".to_string(),
            ],
        );
        identifiers.insert(
            "humboldt".to_string(),
            vec!["humboldt".to_string(), "humbolt_reader".to_string()],
        );

        let result = detect_cross_project(&deps, &identifiers);
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].from_project, "san_luis");
        assert_eq!(result.links[0].to_project, "humboldt");
        assert_eq!(result.links[0].via_dependency, "humboldt_client");
    }
}
