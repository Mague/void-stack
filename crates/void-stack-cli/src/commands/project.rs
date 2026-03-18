use std::path::Path;

use anyhow::{Context, Result, bail};

use void_stack_core::config;
use void_stack_core::global_config::{
    self, default_command_for_dir, find_project, load_global_config, remove_project,
    save_global_config, scan_subprojects,
};
use void_stack_core::model::*;

// ── Add project ──────────────────────────────────────────────

pub fn cmd_add(name: &str, path: &str, wsl: bool, distro: Option<&str>) -> Result<()> {
    let mut config = load_global_config()?;

    if find_project(&config, name).is_some() {
        bail!(
            "Project '{}' already exists. Use 'void remove {}' first.",
            name,
            name
        );
    }

    let default_target = if wsl { Target::Wsl } else { Target::Windows };

    // Resolve path: for WSL → UNC path, for Windows → canonicalize
    let scan_path = if wsl {
        resolve_wsl_path(path, distro)
    } else {
        std::fs::canonicalize(path).with_context(|| format!("Path not found: {}", path))?
    };

    let detected = scan_subprojects(&scan_path);
    let services: Vec<Service> = if detected.is_empty() {
        println!("No services auto-detected. Add them manually with 'void add-service'.");
        vec![]
    } else {
        println!(
            "Detected {} service(s) in {}:",
            detected.len(),
            scan_path.display()
        );
        detected
            .iter()
            .enumerate()
            .map(|(i, (sub_name, sub_path, pt))| {
                let svc_name = sub_name.replace(['/', '\\'], "-");
                let cmd = default_command_for_dir(*pt, sub_path);
                println!(
                    "  {}. {} ({:?}) → {}",
                    i + 1,
                    svc_name,
                    pt,
                    sub_path.display()
                );
                Service {
                    name: svc_name,
                    command: cmd,
                    target: default_target,
                    working_dir: Some(sub_path.to_string_lossy().to_string()),
                    enabled: true,
                    env_vars: vec![],
                    depends_on: vec![],
                    docker: None,
                }
            })
            .collect()
    };

    let project_path = scan_path.to_string_lossy().to_string();

    let project = Project {
        name: name.to_string(),
        description: String::new(),
        path: project_path,
        project_type: None,
        tags: vec![],
        services,
        hooks: None,
    };

    config.projects.push(project);
    save_global_config(&config)?;
    println!(
        "\nProject '{}' added. Edit services with 'void add-service' or edit the config directly.",
        name
    );
    println!("Config: {}", global_config::global_config_path()?.display());

    Ok(())
}

// ── Add service to project ───────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn cmd_add_service(
    project_name: &str,
    svc_name: &str,
    command: &str,
    dir: &str,
    target: &str,
    ports: &[String],
    volumes: &[String],
    docker_args: &[String],
) -> Result<()> {
    let mut config = load_global_config()?;

    let project = config
        .projects
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(project_name))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Project '{}' not found. Use 'void add' first.",
                project_name
            )
        })?;

    if project.services.iter().any(|s| s.name == svc_name) {
        bail!(
            "Service '{}' already exists in project '{}'.",
            svc_name,
            project_name
        );
    }

    let target_enum = match target.to_lowercase().as_str() {
        "windows" => Target::Windows,
        "wsl" => Target::Wsl,
        "docker" => Target::Docker,
        "ssh" => Target::Ssh,
        _ => bail!(
            "Invalid target '{}'. Use: windows, wsl, docker, ssh",
            target
        ),
    };

    let abs_dir = std::fs::canonicalize(dir)
        .with_context(|| format!("Directory not found: {}", dir))?
        .to_string_lossy()
        .to_string();

    // Build Docker config if target is docker and any docker options provided
    let docker = if target_enum == Target::Docker
        && (!ports.is_empty() || !volumes.is_empty() || !docker_args.is_empty())
    {
        Some(DockerConfig {
            ports: ports.to_vec(),
            volumes: volumes.to_vec(),
            extra_args: docker_args.to_vec(),
        })
    } else {
        None
    };

    project.services.push(Service {
        name: svc_name.to_string(),
        command: command.to_string(),
        target: target_enum,
        working_dir: Some(abs_dir.clone()),
        enabled: true,
        env_vars: vec![],
        depends_on: vec![],
        docker,
    });

    save_global_config(&config)?;
    println!(
        "Service '{}' added to '{}' (target: {}, dir: {})",
        svc_name, project_name, target_enum, abs_dir
    );
    if target_enum == Target::Docker {
        if !ports.is_empty() {
            println!("  ports: {}", ports.join(", "));
        }
        if !volumes.is_empty() {
            println!("  volumes: {}", volumes.join(", "));
        }
    }

    Ok(())
}

// ── Remove project ───────────────────────────────────────────

pub fn cmd_remove(name: &str) -> Result<()> {
    let mut config = load_global_config()?;
    if remove_project(&mut config, name) {
        save_global_config(&config)?;
        println!("Project '{}' removed.", name);
    } else {
        println!("Project '{}' not found.", name);
    }
    Ok(())
}

// ── List projects ────────────────────────────────────────────

pub fn cmd_list() -> Result<()> {
    let config = load_global_config()?;

    if config.projects.is_empty() {
        println!("No projects registered.");
        println!("Add one with: void add <name> <path>");
        return Ok(());
    }

    for project in &config.projects {
        println!("{}", project.name);
        println!("  path: {}", project.path);
        if project.services.is_empty() {
            println!("  (no services)");
        } else {
            for svc in &project.services {
                let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
                println!(
                    "  {} [{}] {} → {}",
                    if svc.enabled { "●" } else { "○" },
                    svc.target,
                    svc.name,
                    dir,
                );
                println!("    cmd: {}", svc.command);
                if let Some(ref docker) = svc.docker {
                    if !docker.ports.is_empty() {
                        println!("    ports: {}", docker.ports.join(", "));
                    }
                    if !docker.volumes.is_empty() {
                        println!("    volumes: {}", docker.volumes.join(", "));
                    }
                    if !docker.extra_args.is_empty() {
                        println!("    docker args: {}", docker.extra_args.join(" "));
                    }
                }
            }
        }
        println!();
    }

    Ok(())
}

// ── Scan directory ───────────────────────────────────────────

pub fn cmd_scan(path: &str, wsl: bool, distro: Option<&str>) {
    let scan_path = if wsl {
        resolve_wsl_path(path, distro)
    } else {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.into())
    };

    println!("Scanning {}...\n", scan_path.display());
    let detected = scan_subprojects(&scan_path);
    if detected.is_empty() {
        println!("No projects detected.");
    } else {
        for (name, sub_path, pt) in &detected {
            println!("  {:?} → {} ({})", pt, name, sub_path.display());
        }
        if wsl {
            println!(
                "\nUse 'void add <name> {} --wsl' to register this project.",
                path
            );
        } else {
            println!(
                "\nUse 'void add <name> {}' to register this project.",
                scan_path.display()
            );
        }
    }
}

// ── Read file ────────────────────────────────────────────────

pub fn cmd_read_file(project_name: &str, relative_path: &str) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found", project_name))?;

    let project_path = Path::new(&project.path);
    match void_stack_core::file_reader::read_project_file(project_path, relative_path) {
        Ok(content) => {
            println!("{content}");
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("{e}")),
    }
}

// ── Init (legacy local mode) ─────────────────────────────────

pub fn cmd_init(path: &str) -> Result<()> {
    let dir = Path::new(path);
    let project_type = config::detect_project_type(dir);

    let project = Project {
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "my-project".to_string()),
        description: String::new(),
        path: path.to_string(),
        project_type: Some(project_type),
        tags: vec![],
        services: vec![Service {
            name: "main".to_string(),
            command: default_command_for_dir(project_type, std::path::Path::new(path)),
            target: Target::native(),
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
            docker: None,
        }],
        hooks: Some(HookConfig {
            venv: project_type == ProjectType::Python,
            install_deps: true,
            build: false,
            custom: vec![],
        }),
    };

    config::save_project(&project, dir)?;
    println!(
        "Created void-stack.toml ({:?} project detected)",
        project_type
    );
    Ok(())
}

// ── WSL helpers ──────────────────────────────────────────────

/// Resolve a WSL path to a UNC path that works with Windows std::fs.
/// Handles:
///   - Linux paths: /home/user/project → \\wsl.localhost\<distro>\home\user\project
///   - UNC paths: \\wsl.localhost\Ubuntu\... → passed through (stripped of \\?\UNC\ if present)
///   - Git Bash mangled: C:\Program Files\Git\home\user\... → \\wsl.localhost\<distro>\home\user\...
pub fn resolve_wsl_path(path: &str, distro: Option<&str>) -> std::path::PathBuf {
    use void_stack_core::runner::local::is_wsl_unc_path;

    let d = distro
        .map(|s| s.to_string())
        .or_else(detect_default_wsl_distro)
        .unwrap_or_else(|| "Ubuntu".to_string());

    // Already a proper UNC WSL path
    if is_wsl_unc_path(path) {
        return std::path::PathBuf::from(path);
    }

    // Handle \\?\UNC\wsl.localhost\... (Git Bash converts // to \\?\UNC\)
    if let Some(rest) = path
        .strip_prefix(r"\\?\UNC\wsl.localhost\")
        .or_else(|| path.strip_prefix(r"\\?\UNC\wsl$\"))
    {
        return std::path::PathBuf::from(format!(r"\\wsl.localhost\{}", rest));
    }

    // Git Bash mangles /home/... to C:\Program Files\Git\home\...
    // Detect this pattern: drive letter + path containing \home\ or similar Linux paths
    let normalized = path.replace('/', r"\");
    for linux_root in &[
        r"\home\", r"\opt\", r"\usr\", r"\var\", r"\tmp\", r"\etc\", r"\root\", r"\mnt\",
    ] {
        if let Some(pos) = normalized.find(linux_root) {
            let linux_part = &normalized[pos..].replace('\\', "/");
            return std::path::PathBuf::from(format!(
                r"\\wsl.localhost\{}{}",
                d,
                linux_part.replace('/', r"\")
            ));
        }
    }

    // Pure Linux path starting with /
    if path.starts_with('/') {
        return std::path::PathBuf::from(format!(
            r"\\wsl.localhost\{}{}",
            d,
            path.replace('/', r"\")
        ));
    }

    // Fallback: use as-is
    std::path::PathBuf::from(path)
}

/// Detect the default WSL distribution name.
fn detect_default_wsl_distro() -> Option<String> {
    let output = std::process::Command::new("wsl")
        .args(["--list", "--quiet"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // WSL outputs UTF-16LE on Windows
    let text = String::from_utf16_lossy(
        &output
            .stdout
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<u16>>(),
    );

    // First non-empty line is the default distro
    text.lines()
        .map(|l| l.trim().trim_matches('\0').to_string())
        .find(|l| !l.is_empty())
}
