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

    let mut crate_names: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for member_path in &member_paths {
        let member_toml = root.join(member_path).join("Cargo.toml");
        if let Ok(c) = std::fs::read_to_string(&member_toml)
            && let Ok(v) = c.parse::<toml::Value>()
            && let Some(name) = v
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
        {
            crate_names.insert(name.to_string(), member_path.clone());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_crate_relationships() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["core", "cli"]
"#,
        )
        .unwrap();

        std::fs::create_dir_all(dir.path().join("core")).unwrap();
        std::fs::write(
            dir.path().join("core/Cargo.toml"),
            r#"
[package]
name = "my-core"
version = "0.1.0"
"#,
        )
        .unwrap();

        std::fs::create_dir_all(dir.path().join("cli")).unwrap();
        std::fs::write(
            dir.path().join("cli/Cargo.toml"),
            r#"
[package]
name = "my-cli"
version = "0.1.0"

[dependencies]
my-core = { path = "../core" }
"#,
        )
        .unwrap();

        let links = detect_crate_relationships(dir.path());
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], ("my-cli".to_string(), "my-core".to_string()));
    }

    #[test]
    fn test_detect_no_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[package]
name = "standalone"
version = "0.1.0"
"#,
        )
        .unwrap();

        let links = detect_crate_relationships(dir.path());
        assert!(links.is_empty());
    }

    #[test]
    fn test_detect_no_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let links = detect_crate_relationships(dir.path());
        assert!(links.is_empty());
    }

    #[test]
    fn test_detect_multiple_deps() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["core", "proto", "daemon"]
"#,
        )
        .unwrap();

        for (name, deps) in &[
            ("core", ""),
            ("proto", "[dependencies]\nmy-core = { path = \"../core\" }"),
            (
                "daemon",
                "[dependencies]\nmy-core = { path = \"../core\" }\nmy-proto = { path = \"../proto\" }",
            ),
        ] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
            std::fs::write(
                dir.path().join(format!("{}/Cargo.toml", name)),
                format!(
                    "[package]\nname = \"my-{}\"\nversion = \"0.1.0\"\n\n{}",
                    name, deps
                ),
            )
            .unwrap();
        }

        let links = detect_crate_relationships(dir.path());
        assert!(links.len() >= 3);
        assert!(links.contains(&("my-proto".to_string(), "my-core".to_string())));
        assert!(links.contains(&("my-daemon".to_string(), "my-core".to_string())));
        assert!(links.contains(&("my-daemon".to_string(), "my-proto".to_string())));
    }
}
