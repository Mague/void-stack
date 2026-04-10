use super::GlobalConfig;
use crate::model::Project;

/// Find a project by name in the global config.
pub fn find_project<'a>(config: &'a GlobalConfig, name: &str) -> Option<&'a Project> {
    config
        .projects
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Remove a project by name. Returns true if found and removed.
pub fn remove_project(config: &mut GlobalConfig, name: &str) -> bool {
    let before = config.projects.len();
    config
        .projects
        .retain(|p| !p.name.eq_ignore_ascii_case(name));
    config.projects.len() < before
}

/// Remove a service from a project by name. Returns true if found and removed.
pub fn remove_service(config: &mut GlobalConfig, project_name: &str, service_name: &str) -> bool {
    if let Some(proj) = config
        .projects
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(project_name))
    {
        let before = proj.services.len();
        proj.services
            .retain(|s| !s.name.eq_ignore_ascii_case(service_name));
        proj.services.len() < before
    } else {
        false
    }
}
