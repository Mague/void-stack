pub mod analysis;
pub mod audit;
pub mod board;
pub mod briefing;
pub mod debt;
pub mod dependencies;
pub mod diagrams;
pub mod docker;
pub mod docs;
pub mod editor;
pub mod intel;
pub mod logs;
pub mod projects;
pub mod scan;
#[allow(unused_variables)]
pub mod search;
pub mod services;
pub mod space;
pub mod stats;
pub mod suggest;

/// Shared helpers for the command unit tests. All command functions that read
/// or write the global config resolve it through `VOID_STACK_DATA_DIR`, so the
/// tests point that variable at a per-process tempdir and serialize every
/// config-mutating test on one mutex (the isolated `config.toml` is shared).
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use void_stack_core::global_config::{GlobalConfig, load_global_config, save_global_config};
    use void_stack_core::model::Project;

    /// Point `VOID_STACK_DATA_DIR` at one shared per-process tempdir so config
    /// and other central artifacts never touch the user's real data dir.
    /// Repeated calls converge on the same directory, so parallel tests don't
    /// race on the value.
    pub fn isolate_data_dir() {
        static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
        let dir = DIR.get_or_init(|| tempfile::tempdir().expect("tempdir for test data"));
        // SAFETY: every caller sets the same value, so races are benign.
        unsafe { std::env::set_var("VOID_STACK_DATA_DIR", dir.path()) };
    }

    /// Acquire the config lock, isolate the data dir and reset `config.toml` to
    /// an empty registry. Hold the returned guard for the whole test so no
    /// other config-mutating test observes a half-written registry. Recovers
    /// from poisoning so one failing test never cascades.
    pub fn config_guard() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        isolate_data_dir();
        save_global_config(&GlobalConfig::default()).expect("reset config");
        guard
    }

    /// Register a project in the (already isolated) global config.
    pub fn register(project: Project) {
        let mut cfg = load_global_config().expect("load config");
        cfg.projects.push(project);
        save_global_config(&cfg).expect("save config");
    }

    /// Build a minimal project fixture pointing at `path`.
    pub fn project(name: &str, path: &std::path::Path) -> Project {
        Project {
            name: name.to_string(),
            description: String::new(),
            path: path.to_string_lossy().to_string(),
            project_type: None,
            tags: Vec::new(),
            services: Vec::new(),
            hooks: None,
        }
    }
}
