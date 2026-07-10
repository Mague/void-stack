pub mod ai;

/// Test-only: point `VOID_STACK_DATA_DIR` at a shared per-process tempdir
/// so fixtures never write into the user's real data dir (indexes,
/// contracts caches, stats). Call it first in any test that touches
/// central state; repeated calls converge on the same directory.
#[cfg(test)]
pub(crate) fn isolate_test_data_dir() {
    use std::sync::OnceLock;
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    let dir = DIR.get_or_init(|| tempfile::tempdir().expect("tempdir for test data"));
    // SAFETY: every caller sets the same value, so races are benign.
    unsafe { std::env::set_var(global_config::DATA_DIR_ENV, dir.path()) };
}
pub mod analyzer;
pub mod audit;
pub mod backend;
pub mod board;
pub mod boardhistory;
pub mod bootstrap;
pub mod briefing;
pub mod claudeignore;
pub mod commitmsg;
pub mod config;
pub mod context;
pub mod deadcode;
pub mod detector;
pub mod diagram;
pub mod diff;
pub mod docker;
pub mod doctor;
pub mod envcheck;
pub mod error;
pub mod file_reader;
pub mod fs_util;
pub mod global_config;
pub mod handoff;
pub mod hooks;
pub mod ignore;
pub mod log_filter;
pub mod manager;
pub mod model;
pub mod process_util;
pub mod project_config;
#[cfg(feature = "structural")]
pub mod review;
pub mod runner;
pub mod security;
pub mod space;
pub mod stats;
#[cfg(feature = "structural")]
pub mod structural;
#[cfg(feature = "structural")]
pub mod testing;
pub mod todosync;
#[cfg(feature = "vector")]
pub mod vector_index;
