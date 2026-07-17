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

/// Build the (program, args) tuple for install_deps hook.
/// Returns None if no install is needed for this project type/path.
pub fn build_install_deps_command(path: &Path, pt: ProjectType) -> Option<(String, Vec<String>)> {
    match pt {
        ProjectType::Python => {
            let pip = if path.join(".venv/Scripts/pip.exe").exists() {
                ".venv/Scripts/pip.exe"
            } else if path.join(".venv/bin/pip").exists() {
                ".venv/bin/pip"
            } else {
                "pip"
            };

            if path.join("requirements.txt").exists() {
                Some((
                    pip.to_string(),
                    vec![
                        "install".to_string(),
                        "-r".to_string(),
                        "requirements.txt".to_string(),
                        "-q".to_string(),
                    ],
                ))
            } else if path.join("pyproject.toml").exists() {
                Some((
                    pip.to_string(),
                    vec![
                        "install".to_string(),
                        "-e".to_string(),
                        ".".to_string(),
                        "-q".to_string(),
                    ],
                ))
            } else {
                None
            }
        }
        ProjectType::Node => {
            if path.join("package-lock.json").exists() {
                Some((
                    "npm".to_string(),
                    vec!["ci".to_string(), "--silent".to_string()],
                ))
            } else if path.join("package.json").exists() {
                Some((
                    "npm".to_string(),
                    vec!["install".to_string(), "--silent".to_string()],
                ))
            } else {
                None
            }
        }
        ProjectType::Rust => Some((
            "cargo".to_string(),
            vec!["build".to_string(), "--quiet".to_string()],
        )),
        ProjectType::Go => Some((
            "go".to_string(),
            vec!["mod".to_string(), "download".to_string()],
        )),
        _ => None,
    }
}

/// Build the (program, args) tuple for build hook.
/// Returns None if no build is needed for this project type/path.
pub fn build_build_command(path: &Path, pt: ProjectType) -> Option<(String, Vec<String>)> {
    match pt {
        ProjectType::Rust => Some(("cargo".to_string(), vec!["build".to_string()])),
        ProjectType::Go => Some((
            "go".to_string(),
            vec!["build".to_string(), "./...".to_string()],
        )),
        ProjectType::Node => {
            if path.join("package.json").exists() {
                Some((
                    "npm".to_string(),
                    vec!["run".to_string(), "build".to_string()],
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

async fn run_install_deps_hook(path: &Path, pt: ProjectType) -> Result<()> {
    let (program, args) = match build_install_deps_command(path, pt) {
        Some(cmd) => cmd,
        None => return Ok(()),
    };

    info!(
        hook = "install_deps",
        program = %program,
        "Installing dependencies..."
    );

    let status = Command::new(&program)
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
    let (program, args) = match build_build_command(path, pt) {
        Some(cmd) => cmd,
        None => return Ok(()),
    };

    info!(hook = "build", program = %program, "Building...");

    let status = Command::new(&program)
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

/// Check whether a venv hook should create a new venv.
/// Returns true if project is Python and no .venv exists yet.
pub fn needs_venv(path: &Path, pt: ProjectType) -> bool {
    pt == ProjectType::Python && !path.join(".venv").exists()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_deps_python_requirements() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        let cmd = build_install_deps_command(dir.path(), ProjectType::Python);
        assert!(cmd.is_some());
        let (prog, args) = cmd.unwrap();
        assert_eq!(prog, "pip");
        assert!(args.contains(&"-r".to_string()));
        assert!(args.contains(&"requirements.txt".to_string()));
    }

    #[test]
    fn test_install_deps_python_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\n").unwrap();
        let cmd = build_install_deps_command(dir.path(), ProjectType::Python);
        assert!(cmd.is_some());
        let (prog, args) = cmd.unwrap();
        assert_eq!(prog, "pip");
        assert!(args.contains(&"-e".to_string()));
    }

    #[test]
    fn test_install_deps_python_with_venv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        let venv_dir = dir.path().join(".venv").join("bin");
        std::fs::create_dir_all(&venv_dir).unwrap();
        std::fs::write(venv_dir.join("pip"), "").unwrap();
        let cmd = build_install_deps_command(dir.path(), ProjectType::Python);
        assert!(cmd.is_some());
        let (prog, _) = cmd.unwrap();
        assert_eq!(prog, ".venv/bin/pip");
    }

    #[test]
    fn test_install_deps_node_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        let cmd = build_install_deps_command(dir.path(), ProjectType::Node);
        assert!(cmd.is_some());
        let (prog, args) = cmd.unwrap();
        assert_eq!(prog, "npm");
        assert!(args.contains(&"ci".to_string()));
    }

    #[test]
    fn test_install_deps_node_no_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let cmd = build_install_deps_command(dir.path(), ProjectType::Node);
        assert!(cmd.is_some());
        let (prog, args) = cmd.unwrap();
        assert_eq!(prog, "npm");
        assert!(args.contains(&"install".to_string()));
    }

    #[test]
    fn test_install_deps_rust() {
        let dir = tempfile::tempdir().unwrap();
        let (prog, _) = build_install_deps_command(dir.path(), ProjectType::Rust).unwrap();
        assert_eq!(prog, "cargo");
    }

    #[test]
    fn test_install_deps_go() {
        let dir = tempfile::tempdir().unwrap();
        let (prog, args) = build_install_deps_command(dir.path(), ProjectType::Go).unwrap();
        assert_eq!(prog, "go");
        assert!(args.contains(&"mod".to_string()));
    }

    #[test]
    fn test_install_deps_unsupported_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_install_deps_command(dir.path(), ProjectType::Docker).is_none());
    }

    #[test]
    fn test_install_deps_python_no_files_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_install_deps_command(dir.path(), ProjectType::Python).is_none());
    }

    #[test]
    fn test_build_command_rust() {
        let dir = tempfile::tempdir().unwrap();
        let (prog, args) = build_build_command(dir.path(), ProjectType::Rust).unwrap();
        assert_eq!(prog, "cargo");
        assert_eq!(args, vec!["build"]);
    }

    #[test]
    fn test_build_command_go() {
        let dir = tempfile::tempdir().unwrap();
        let (prog, _) = build_build_command(dir.path(), ProjectType::Go).unwrap();
        assert_eq!(prog, "go");
    }

    #[test]
    fn test_build_command_node_with_package() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let (prog, args) = build_build_command(dir.path(), ProjectType::Node).unwrap();
        assert_eq!(prog, "npm");
        assert!(args.contains(&"build".to_string()));
    }

    #[test]
    fn test_build_command_node_no_package_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_build_command(dir.path(), ProjectType::Node).is_none());
    }

    #[test]
    fn test_build_command_unsupported_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_build_command(dir.path(), ProjectType::Python).is_none());
    }

    #[test]
    fn test_needs_venv_python_no_venv() {
        let dir = tempfile::tempdir().unwrap();
        assert!(needs_venv(dir.path(), ProjectType::Python));
    }

    #[test]
    fn test_needs_venv_python_with_existing_venv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".venv")).unwrap();
        assert!(!needs_venv(dir.path(), ProjectType::Python));
    }

    #[test]
    fn test_needs_venv_non_python() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!needs_venv(dir.path(), ProjectType::Rust));
        assert!(!needs_venv(dir.path(), ProjectType::Node));
    }

    #[test]
    fn test_install_deps_python_windows_venv_pip() {
        // The `.venv/Scripts/pip.exe` branch is checked before `.venv/bin/pip`
        // and is selected on any OS when that file exists.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        let scripts = dir.path().join(".venv").join("Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("pip.exe"), "").unwrap();
        let (prog, args) = build_install_deps_command(dir.path(), ProjectType::Python).unwrap();
        assert_eq!(prog, ".venv/Scripts/pip.exe");
        assert!(args.contains(&"requirements.txt".to_string()));
    }

    #[tokio::test]
    async fn test_run_pre_launch_no_hooks_ok() {
        // All auto hooks disabled and no custom hooks → nothing runs, Ok.
        let dir = tempfile::tempdir().unwrap();
        let config = HookConfig::default();
        let res = run_pre_launch(
            &config,
            dir.path().to_str().unwrap(),
            Some(ProjectType::Rust),
        )
        .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_run_pre_launch_no_project_type_skips_auto() {
        // With no project type, the auto-hook block is skipped even if enabled.
        let dir = tempfile::tempdir().unwrap();
        let config = HookConfig {
            venv: true,
            install_deps: true,
            build: true,
            ..Default::default()
        };
        let res = run_pre_launch(&config, dir.path().to_str().unwrap(), None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_run_pre_launch_custom_success() {
        // `echo` succeeds under both cmd and sh → custom hook returns Ok.
        let dir = tempfile::tempdir().unwrap();
        let config = HookConfig {
            custom: vec!["echo hello".to_string()],
            ..Default::default()
        };
        let res = run_pre_launch(&config, dir.path().to_str().unwrap(), None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_run_pre_launch_custom_failure() {
        // A missing command makes the shell exit non-zero → HookFailed.
        let dir = tempfile::tempdir().unwrap();
        let config = HookConfig {
            custom: vec!["this_command_should_not_exist_xyz_123".to_string()],
            ..Default::default()
        };
        let res = run_pre_launch(&config, dir.path().to_str().unwrap(), None).await;
        assert!(res.is_err());
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("custom"),
            "expected custom hook error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_run_pre_launch_venv_skipped_when_exists() {
        // Python project with a pre-existing .venv → venv hook returns early,
        // never spawning python.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".venv")).unwrap();
        let config = HookConfig {
            venv: true,
            ..Default::default()
        };
        let res = run_pre_launch(
            &config,
            dir.path().to_str().unwrap(),
            Some(ProjectType::Python),
        )
        .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_run_pre_launch_venv_non_python_noop() {
        // venv hook returns Ok early for non-Python project types.
        let dir = tempfile::tempdir().unwrap();
        let config = HookConfig {
            venv: true,
            ..Default::default()
        };
        let res = run_pre_launch(
            &config,
            dir.path().to_str().unwrap(),
            Some(ProjectType::Rust),
        )
        .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_run_pre_launch_install_deps_none_noop() {
        // Python with no requirements/pyproject → install command is None → early Ok.
        let dir = tempfile::tempdir().unwrap();
        let config = HookConfig {
            install_deps: true,
            ..Default::default()
        };
        let res = run_pre_launch(
            &config,
            dir.path().to_str().unwrap(),
            Some(ProjectType::Python),
        )
        .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_run_pre_launch_build_none_noop() {
        // Node without package.json → build command is None → early Ok.
        let dir = tempfile::tempdir().unwrap();
        let config = HookConfig {
            build: true,
            ..Default::default()
        };
        let res = run_pre_launch(
            &config,
            dir.path().to_str().unwrap(),
            Some(ProjectType::Node),
        )
        .await;
        assert!(res.is_ok());
    }
}
