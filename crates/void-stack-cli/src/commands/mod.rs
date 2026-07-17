pub mod analysis;
pub mod board;
pub mod bootstrap;
pub mod briefing;
pub mod commit;
pub mod context;
#[cfg(feature = "vector")]
pub mod contracts;
pub mod daemon;
pub mod deps;
pub mod docker;
pub mod doctor;
pub mod env;
pub mod handoff;
pub mod project;
pub mod service;
pub mod setup;
pub mod stats;

/// Shared fixtures for the command unit tests.
///
/// Everything that touches the central registry goes through the
/// `VOID_STACK_DATA_DIR` isolation described in
/// `crates/void-stack-core/tests/README.md`: one per-process tempdir so
/// fixtures never write into the user's real data dir. Because the
/// isolated `config.toml` is still a single shared file, tests that read
/// or write it must hold [`testutil::config_lock`] for their whole body.
#[cfg(test)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use void_stack_core::global_config::{load_global_config, save_global_config};
    use void_stack_core::model::Project;

    /// Point `VOID_STACK_DATA_DIR` at one shared per-process tempdir.
    /// Repeated calls converge on the same directory.
    pub fn isolate_data_dir() {
        static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
        let dir = DIR.get_or_init(|| tempfile::tempdir().expect("tempdir for test data"));
        // SAFETY: every caller sets the same value, so races are benign.
        unsafe { std::env::set_var(void_stack_core::global_config::DATA_DIR_ENV, dir.path()) };
    }

    /// Serialize tests that read/write the shared (isolated) global
    /// config — `save_global_config` rewrites the whole file, so
    /// concurrent load/save from parallel tests would race.
    pub fn config_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Unique fixture-project name; the `-fixture-` infix is what
    /// `void doctor --fix` recognizes for batch cleanup of leftovers.
    pub fn unique_name(area: &str) -> String {
        static N: AtomicUsize = AtomicUsize::new(0);
        format!(
            "cli-{}-fixture-{}-{}",
            area,
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// Register a fixture project rooted at `root` in the isolated
    /// registry. Caller must hold [`config_lock`].
    pub fn register_project(name: &str, root: &Path) {
        isolate_data_dir();
        let mut config = load_global_config().expect("load isolated config");
        if !config.projects.iter().any(|p| p.name == name) {
            config.projects.push(Project {
                name: name.to_string(),
                description: "CLI test fixture".to_string(),
                path: root.to_string_lossy().into_owned(),
                project_type: None,
                tags: vec![],
                services: vec![],
                hooks: None,
            });
            save_global_config(&config).expect("save isolated config");
        }
    }

    /// Run git in `dir`, asserting success.
    pub fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Fresh git repo fixture (identity + gpgsign configured).
    pub fn git_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "t@t.io"]);
        git(&root, &["config", "user.name", "t"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        (tmp, root)
    }
}
