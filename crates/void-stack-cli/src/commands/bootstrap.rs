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
