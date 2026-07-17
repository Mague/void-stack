//! `void bootstrap` — export/import the registry for new machines.

use std::path::PathBuf;

use anyhow::Result;
use void_stack_core::bootstrap;
use void_stack_core::global_config::{load_global_config, save_global_config};

pub fn cmd_bootstrap_export(out: Option<&str>, root: Option<&str>) -> Result<()> {
    let config = load_global_config()?;
    let root = match root {
        Some(r) => PathBuf::from(r),
        None => dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home dir"))?,
    };
    let portable = bootstrap::export_registry(&config, &root);
    let toml_str = bootstrap::registry_to_toml(&portable).map_err(|e| anyhow::anyhow!(e))?;

    let out_path = out.unwrap_or("registry.toml");
    std::fs::write(out_path, &toml_str)?;
    println!(
        "✓ exported {} project(s) to {} (root: {})",
        portable.projects.len(),
        out_path,
        root.display()
    );
    println!("  no secrets exported — env_vars and docker extra_args stay on this machine");
    Ok(())
}

pub fn cmd_bootstrap_import(file: &str, root: Option<&str>) -> Result<()> {
    let content = std::fs::read_to_string(file)?;
    let portable = bootstrap::registry_from_toml(&content).map_err(|e| anyhow::anyhow!(e))?;
    let root = match root {
        Some(r) => PathBuf::from(r),
        None => dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home dir"))?,
    };

    let mut config = load_global_config()?;
    let report = bootstrap::import_registry(&mut config, &portable, &root);
    if !report.imported.is_empty() {
        save_global_config(&config)?;
    }

    println!(
        "✓ imported {} project(s) (root: {})",
        report.imported.len(),
        root.display()
    );
    for name in &report.imported {
        println!("  + {}", name);
    }
    for name in &report.already_registered {
        println!("  = {} (already registered, untouched)", name);
    }
    for (name, path) in &report.missing {
        println!("  ✗ {} — path not found on this machine: {}", name, path);
    }
    if !report.missing.is_empty() {
        println!(
            "\n{} project(s) skipped — clone them and re-run, or pass a different --root",
            report.missing.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil;

    #[test]
    fn test_bootstrap_import_missing_file_errors() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();
        assert!(cmd_bootstrap_import("Z:/definitely/not/here/registry.toml", None).is_err());
    }

    #[test]
    fn test_bootstrap_import_invalid_toml_errors() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("registry.toml");
        std::fs::write(&bad, "this is { not toml").unwrap();
        assert!(cmd_bootstrap_import(&bad.to_string_lossy(), None).is_err());
    }

    #[test]
    fn test_bootstrap_export_then_import_roundtrip() {
        let _guard = testutil::config_lock();
        // Workspace layout: <ws>/proj is the registered project dir.
        let ws = tempfile::tempdir().unwrap();
        let proj_dir = ws.path().join("proj");
        std::fs::create_dir_all(&proj_dir).unwrap();
        let name = testutil::unique_name("bootstrap");
        testutil::register_project(&name, &proj_dir);

        let out = ws.path().join("registry.toml");
        cmd_bootstrap_export(
            Some(&out.to_string_lossy()),
            Some(&ws.path().to_string_lossy()),
        )
        .unwrap();
        let toml_str = std::fs::read_to_string(&out).unwrap();
        assert!(toml_str.contains(&name), "exported registry:\n{toml_str}");

        // Re-import over the same registry: everything already there.
        cmd_bootstrap_import(&out.to_string_lossy(), Some(&ws.path().to_string_lossy())).unwrap();

        // Drop the project from the registry, import again → restored.
        use void_stack_core::global_config::{load_global_config, save_global_config};
        let mut config = load_global_config().unwrap();
        config.projects.retain(|p| p.name != name);
        save_global_config(&config).unwrap();

        cmd_bootstrap_import(&out.to_string_lossy(), Some(&ws.path().to_string_lossy())).unwrap();
        let config = load_global_config().unwrap();
        assert!(
            config.projects.iter().any(|p| p.name == name),
            "project must be re-registered after import"
        );
    }
}
