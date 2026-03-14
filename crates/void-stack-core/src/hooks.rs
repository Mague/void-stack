use std::path::Path;
use tokio::process::Command;
use tracing::info;

use crate::error::{Result, VoidStackError};
use crate::model::{HookConfig, ProjectType};
use crate::process_util::{HideWindow, shell_command};

/// Run pre-launch hooks based on project type and config.
pub async fn run_pre_launch(
    config: &HookConfig,
    project_path: &str,
    project_type: Option<ProjectType>,
) -> Result<()> {
    let path = Path::new(project_path);

    // Auto hooks based on project type
    if let Some(pt) = project_type {
        if config.venv {
            run_venv_hook(path, pt).await?;
        }
        if config.install_deps {
            run_install_deps_hook(path, pt).await?;
        }
        if config.build {
            run_build_hook(path, pt).await?;
        }
    }

    // Custom hooks
    for cmd_str in &config.custom {
        run_custom_hook(path, cmd_str).await?;
    }

    Ok(())
}

async fn run_venv_hook(path: &Path, pt: ProjectType) -> Result<()> {
    if pt != ProjectType::Python {
        return Ok(());
    }

    let venv_path = path.join(".venv");
    if venv_path.exists() {
        info!("Python venv already exists");
        return Ok(());
    }

    info!("Creating Python venv...");
    let status = Command::new("python")
        .args(["-m", "venv", ".venv"])
        .current_dir(path)
        .hide_window()
        .status()
        .await
        .map_err(|e| VoidStackError::HookFailed {
            hook: "venv".to_string(),
            reason: e.to_string(),
        })?;

    if !status.success() {
        return Err(VoidStackError::HookFailed {
            hook: "venv".to_string(),
            reason: format!("exited with code {:?}", status.code()),
        });
    }

    Ok(())
}

async fn run_install_deps_hook(path: &Path, pt: ProjectType) -> Result<()> {
    let (program, args): (&str, Vec<&str>) = match pt {
        ProjectType::Python => {
            // Use venv pip if available
            let pip = if path.join(".venv/Scripts/pip.exe").exists() {
                ".venv/Scripts/pip.exe"
            } else if path.join(".venv/bin/pip").exists() {
                ".venv/bin/pip"
            } else {
                "pip"
            };

            if path.join("requirements.txt").exists() {
                (pip, vec!["install", "-r", "requirements.txt", "-q"])
            } else if path.join("pyproject.toml").exists() {
                (pip, vec!["install", "-e", ".", "-q"])
            } else {
                return Ok(());
            }
        }
        ProjectType::Node => {
            if path.join("package-lock.json").exists() {
                ("npm", vec!["ci", "--silent"])
            } else if path.join("package.json").exists() {
                ("npm", vec!["install", "--silent"])
            } else {
                return Ok(());
            }
        }
        ProjectType::Rust => ("cargo", vec!["build", "--quiet"]),
        ProjectType::Go => ("go", vec!["mod", "download"]),
        _ => return Ok(()),
    };

    info!(
        hook = "install_deps",
        program = program,
        "Installing dependencies..."
    );

    let status = Command::new(program)
        .args(&args)
        .current_dir(path)
        .hide_window()
        .status()
        .await
        .map_err(|e| VoidStackError::HookFailed {
            hook: "install_deps".to_string(),
            reason: e.to_string(),
        })?;

    if !status.success() {
        return Err(VoidStackError::HookFailed {
            hook: "install_deps".to_string(),
            reason: format!("{} exited with code {:?}", program, status.code()),
        });
    }

    Ok(())
}

async fn run_build_hook(path: &Path, pt: ProjectType) -> Result<()> {
    let (program, args): (&str, Vec<&str>) = match pt {
        ProjectType::Rust => ("cargo", vec!["build"]),
        ProjectType::Go => ("go", vec!["build", "./..."]),
        ProjectType::Node => {
            if path.join("package.json").exists() {
                ("npm", vec!["run", "build"])
            } else {
                return Ok(());
            }
        }
        _ => return Ok(()),
    };

    info!(hook = "build", program = program, "Building...");

    let status = Command::new(program)
        .args(&args)
        .current_dir(path)
        .hide_window()
        .status()
        .await
        .map_err(|e| VoidStackError::HookFailed {
            hook: "build".to_string(),
            reason: e.to_string(),
        })?;

    if !status.success() {
        return Err(VoidStackError::HookFailed {
            hook: "build".to_string(),
            reason: format!("{} exited with code {:?}", program, status.code()),
        });
    }

    Ok(())
}

async fn run_custom_hook(path: &Path, cmd_str: &str) -> Result<()> {
    info!(hook = "custom", command = cmd_str, "Running custom hook...");

    let status = shell_command(cmd_str)
        .current_dir(path)
        .hide_window()
        .status()
        .await
        .map_err(|e| VoidStackError::HookFailed {
            hook: format!("custom: {}", cmd_str),
            reason: e.to_string(),
        })?;

    if !status.success() {
        return Err(VoidStackError::HookFailed {
            hook: format!("custom: {}", cmd_str),
            reason: format!("exited with code {:?}", status.code()),
        });
    }

    Ok(())
}
