use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use devlaunch_core::global_config::find_project;
use devlaunch_core::manager::ProcessManager;
use devlaunch_core::model::Project;

pub struct AppState {
    pub managers: Arc<Mutex<HashMap<String, Arc<ProcessManager>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            managers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn find_project(config: &devlaunch_core::global_config::GlobalConfig, name: &str) -> Result<Project, String> {
        find_project(config, name)
            .cloned()
            .ok_or_else(|| format!("Proyecto '{}' no encontrado", name))
    }

    pub async fn get_manager(&self, project: &Project) -> Arc<ProcessManager> {
        let mut managers = self.managers.lock().await;
        if let Some(mgr) = managers.get(&project.name) {
            return Arc::clone(mgr);
        }
        let mgr = Arc::new(ProcessManager::new(project.clone()));
        managers.insert(project.name.clone(), Arc::clone(&mgr));
        mgr
    }
}
