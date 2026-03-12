//! Rust crate dependency relationship detection from Cargo.toml workspace.

use std::path::Path;

/// Detect internal crate dependency relationships from Cargo.toml workspace.
pub(super) fn detect_crate_relationships(root: &Path) -> Vec<(String, String)> {
    let workspace_toml = root.join("Cargo.toml");
    let content = match std::fs::read_to_string(&workspace_toml) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let parsed: toml::Value = match content.parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let members = match parsed
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    {
        Some(m) => m,
        None => return Vec::new(),
    };

    let member_paths: Vec<String> = members
        .iter()
        .filter_map(|m| m.as_str().map(|s| s.to_string()))
        .collect();

    let mut crate_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for member_path in &member_paths {
        let member_toml = root.join(member_path).join("Cargo.toml");
        if let Ok(c) = std::fs::read_to_string(&member_toml) {
            if let Ok(v) = c.parse::<toml::Value>() {
                if let Some(name) = v
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                {
                    crate_names.insert(name.to_string(), member_path.clone());
                }
            }
        }
    }

    let mut links: Vec<(String, String)> = Vec::new();
    for member_path in &member_paths {
        let member_toml = root.join(member_path).join("Cargo.toml");
        let content = match std::fs::read_to_string(&member_toml) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let parsed: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let crate_name = match parsed
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        {
            Some(n) => n.to_string(),
            None => continue,
        };

        for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(deps) = parsed.get(section).and_then(|d| d.as_table()) {
                for dep_name in deps.keys() {
                    if crate_names.contains_key(dep_name) && *dep_name != crate_name {
                        let link = (crate_name.clone(), dep_name.clone());
                        if !links.contains(&link) {
                            links.push(link);
                        }
                    }
                }
            }
        }
    }

    links
}
