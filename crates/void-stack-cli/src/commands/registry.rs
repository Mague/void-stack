//! Project and service registry commands: `add`, `add-service`,
//! `edit-service`, `remove-service`, `remove`, `edit`, `list`.

use anyhow::{Context, Result, bail};
use clap::Args;

use void_stack_core::global_config::{
    ServiceUpdate, edit_service, load_global_config, remove_service, save_global_config,
};

use super::project;
use crate::Commands;

/// `void edit-service`: only the options passed change.
#[derive(Args, Debug)]
pub struct EditServiceArgs {
    /// Project name
    pub project: String,
    /// Current service name
    pub service: String,
    /// New service name
    #[arg(long = "rename")]
    pub new_name: Option<String>,
    /// New command
    #[arg(short, long)]
    pub command: Option<String>,
    /// New working directory
    #[arg(short = 'd', long)]
    pub dir: Option<String>,
    /// New target: windows, macos, wsl, docker, ssh
    #[arg(short, long)]
    pub target: Option<String>,
    /// Enable the service
    #[arg(long, conflicts_with = "disable")]
    pub enable: bool,
    /// Disable the service (start skips it)
    #[arg(long)]
    pub disable: bool,
    /// Docker port mappings, replacing the current ones. Repeatable.
    #[arg(long = "port", num_args = 1)]
    pub ports: Vec<String>,
    /// Docker volume mounts, replacing the current ones. Repeatable.
    #[arg(long = "volume", num_args = 1)]
    pub volumes: Vec<String>,
    /// Extra docker run arguments, replacing the current ones. Repeatable.
    #[arg(long = "docker-arg", num_args = 1)]
    pub docker_args: Vec<String>,
}

/// Dispatch one registry command.
pub fn handle_registry(command: &Commands) -> Result<()> {
    match command {
        Commands::Add {
            name,
            path,
            wsl,
            distro,
        } => project::cmd_add(name, path, *wsl, distro.as_deref()),
        Commands::AddService {
            project: proj,
            name,
            command,
            dir,
            target,
            ports,
            volumes,
            docker_args,
        } => project::cmd_add_service(
            proj,
            name,
            command,
            dir,
            target,
            ports,
            volumes,
            docker_args,
        ),
        Commands::EditService(args) => cmd_edit_service(args),
        Commands::RemoveService {
            project: proj,
            service,
        } => cmd_remove_service(proj, service),
        Commands::Remove { name } => project::cmd_remove(name),
        Commands::Edit {
            name,
            new_name,
            new_path,
        } => project::cmd_edit(name, new_name.as_deref(), new_path.as_deref()),
        Commands::List => project::cmd_list(),
        _ => unreachable!("handle_registry only receives registry commands"),
    }
}

/// A list flag given on the command line replaces the stored list.
fn given(list: &[String]) -> Option<Vec<String>> {
    (!list.is_empty()).then(|| list.to_vec())
}

fn cmd_edit_service(args: &EditServiceArgs) -> Result<()> {
    let working_dir = match &args.dir {
        Some(d) => Some(
            std::fs::canonicalize(d)
                .with_context(|| format!("Directory not found: {}", d))?
                .to_string_lossy()
                .to_string(),
        ),
        None => None,
    };
    let update = ServiceUpdate {
        name: args.new_name.clone(),
        command: args.command.clone(),
        working_dir,
        enabled: (args.enable || args.disable).then_some(args.enable),
        ..Default::default()
    }
    .target_named(args.target.as_deref())
    .map_err(anyhow::Error::msg)?
    .docker_lists(
        given(&args.ports),
        given(&args.volumes),
        given(&args.docker_args),
    );
    let saved = edit_service(&args.project, &args.service, update).map_err(anyhow::Error::msg)?;
    println!(
        "Service '{}' updated in '{}': {} (target: {}, dir: {}{})",
        saved.name,
        args.project,
        saved.command,
        saved.target,
        saved.working_dir.as_deref().unwrap_or("(project root)"),
        if saved.enabled { "" } else { ", disabled" }
    );
    println!(
        "  Restart it if it is running: void stop/start {} {}",
        args.project, saved.name
    );
    Ok(())
}

fn cmd_remove_service(project_name: &str, svc_name: &str) -> Result<()> {
    let mut config = load_global_config()?;
    if !remove_service(&mut config, project_name, svc_name) {
        bail!(
            "Service '{}' not found in project '{}'.",
            svc_name,
            project_name
        );
    }
    save_global_config(&config)?;
    println!("Service '{}' removed from '{}'.", svc_name, project_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use crate::commands::testutil::{config_lock, register_project, unique_name};
    use clap::Parser;
    use void_stack_core::model::{Service, Target};

    fn edit_args(argv: &[&str]) -> EditServiceArgs {
        match Cli::try_parse_from(argv).unwrap().command {
            Commands::EditService(args) => args,
            _ => panic!("expected EditService"),
        }
    }

    #[test]
    fn test_parse_edit_service() {
        let args = edit_args(&[
            "void",
            "edit-service",
            "myproj",
            "web",
            "-c",
            "pnpm dev",
            "-t",
            "wsl",
            "--rename",
            "frontend",
            "--disable",
        ]);
        assert_eq!(
            (args.project.as_str(), args.service.as_str()),
            ("myproj", "web")
        );
        assert_eq!(args.new_name.as_deref(), Some("frontend"));
        assert_eq!(args.command.as_deref(), Some("pnpm dev"));
        assert_eq!(args.target.as_deref(), Some("wsl"));
        assert!(!args.enable && args.disable);
        assert_eq!(args.dir, None, "options not passed stay None");
        assert!(
            Cli::try_parse_from(["void", "edit-service", "p", "s", "--enable", "--disable"])
                .is_err(),
            "--enable and --disable conflict"
        );
    }

    #[test]
    fn test_edit_and_remove_service() {
        let _guard = config_lock();
        let dir = tempfile::tempdir().unwrap();
        let name = unique_name("edit-service");
        register_project(&name, dir.path());
        let mut config = load_global_config().unwrap();
        let project = config.projects.iter_mut().find(|p| p.name == name).unwrap();
        project.services.push(Service {
            name: "web".into(),
            command: "npm run dev".into(),
            target: Target::Windows,
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
            docker: None,
        });
        save_global_config(&config).unwrap();

        cmd_edit_service(&edit_args(&[
            "void",
            "edit-service",
            &name,
            "web",
            "-c",
            "pnpm dev",
            "-t",
            "wsl",
        ]))
        .unwrap();
        let saved = load_global_config().unwrap();
        let svc = &saved
            .projects
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .services[0];
        assert_eq!(
            (svc.command.as_str(), svc.target),
            ("pnpm dev", Target::Wsl)
        );
        assert!(
            cmd_edit_service(&edit_args(&[
                "void",
                "edit-service",
                &name,
                "web",
                "-t",
                "mars"
            ]))
            .is_err()
        );

        cmd_remove_service(&name, "web").unwrap();
        assert!(cmd_remove_service(&name, "web").is_err());
    }
}
