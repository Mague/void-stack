//! Background indexing jobs and their status registry.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use super::index_project;
use crate::model::Project;
use crate::vector_index::stats::IndexStats;

/// Global job registry for background indexing: project_path → status
static INDEX_JOBS: OnceLock<Mutex<HashMap<String, IndexJobStatus>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
pub enum IndexJobStatus {
    Running {
        files_processed: usize,
        files_total: usize,
    },
    Completed {
        stats: IndexStats,
    },
    Failed {
        error: String,
    },
}

pub(crate) fn job_registry() -> &'static Mutex<HashMap<String, IndexJobStatus>> {
    INDEX_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Update job status, recovering from poison if needed.
pub(crate) fn update_job(key: &str, status: IndexJobStatus) {
    let registry = job_registry();
    match registry.lock() {
        Ok(mut jobs) => {
            jobs.insert(key.to_string(), status);
        }
        Err(poisoned) => {
            poisoned.into_inner().insert(key.to_string(), status);
        }
    }
}

/// Read job status, recovering from poison if needed.
pub(crate) fn read_job(key: &str) -> Option<IndexJobStatus> {
    let registry = job_registry();
    match registry.lock() {
        Ok(jobs) => jobs.get(key).cloned(),
        Err(poisoned) => poisoned.into_inner().get(key).cloned(),
    }
}

// ── Public API ──────────────────────────────────────────────

/// Start indexing in a background thread. Returns immediately with job key.
/// `git_base` enables git-diff-based change detection (e.g. "HEAD", "HEAD~1", "main").
pub fn index_project_background(
    project: &Project,
    force: bool,
    git_base: Option<String>,
) -> String {
    let job_key = project.path.clone();

    // Mark as running immediately (poison-safe)
    update_job(
        &job_key,
        IndexJobStatus::Running {
            files_processed: 0,
            files_total: 0,
        },
    );

    let project_clone = project.clone();
    let key = job_key.clone();

    std::thread::spawn(move || {
        let result = index_project(
            &project_clone,
            force,
            git_base.as_deref(),
            |processed, total| {
                update_job(
                    &key,
                    IndexJobStatus::Running {
                        files_processed: processed,
                        files_total: total,
                    },
                );
            },
        );

        match result {
            Ok(stats) => {
                update_job(&key, IndexJobStatus::Completed { stats });
            }
            Err(e) => {
                update_job(
                    &key,
                    IndexJobStatus::Failed {
                        error: e.to_string(),
                    },
                );
            }
        }
    });

    job_key
}

/// Get the current status of an indexing job (poison-safe).
pub fn get_index_job_status(project: &Project) -> Option<IndexJobStatus> {
    read_job(&project.path)
}

/// Like [`index_project_background`] but chains an incremental structural
/// graph build after the re-index completes — sequential on the same worker
/// thread, so it never overlaps with `MAX_CONCURRENT_INDEXING` slots.
/// Keeps BOTH indexes fresh from one trigger (the watcher).
pub fn index_project_background_then_structural(
    project: &Project,
    force: bool,
    git_base: Option<String>,
) -> String {
    let job_key = project.path.clone();

    update_job(
        &job_key,
        IndexJobStatus::Running {
            files_processed: 0,
            files_total: 0,
        },
    );

    let project_clone = project.clone();
    let key = job_key.clone();

    std::thread::spawn(move || {
        let result = index_project(
            &project_clone,
            force,
            git_base.as_deref(),
            |processed, total| {
                update_job(
                    &key,
                    IndexJobStatus::Running {
                        files_processed: processed,
                        files_total: total,
                    },
                );
            },
        );

        match result {
            Ok(stats) => {
                update_job(&key, IndexJobStatus::Completed { stats });
                // Incremental (SHA-256-gated) structural refresh. Failures
                // are logged, never fatal — semantic index is already saved.
                #[cfg(feature = "structural")]
                if let Err(e) = crate::structural::build_structural_graph(&project_clone, false) {
                    tracing::warn!(project = %project_clone.name, error = %e,
                        "structural graph refresh after re-index failed");
                }
            }
            Err(e) => {
                update_job(
                    &key,
                    IndexJobStatus::Failed {
                        error: e.to_string(),
                    },
                );
            }
        }
    });

    job_key
}

// ── Concurrency ─────────────────────────────────────────────

/// Maximum simultaneous indexing operations. The dedicated rayon pool below
/// already caps parallelism at cpus/2; running two jobs on top of it just
/// thrashes the ONNX model cache and SQLite without finishing any faster.
static ACTIVE_INDEXING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const MAX_CONCURRENT_INDEXING: usize = 1;

/// Fraction of logical CPUs to use for indexing (50%, min 2, max 6).
/// Leaves headroom for the OS, services, and fastembed's own threads.
pub(crate) fn indexing_rayon_threads() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    (cpus / 2).clamp(2, 6)
}

/// Dedicated rayon thread pool for indexing.
/// Separate from the global pool so indexing doesn't starve other work.
static INDEXING_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

pub(super) fn indexing_pool() -> &'static rayon::ThreadPool {
    INDEXING_POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(indexing_rayon_threads())
            .thread_name(|i| format!("void-index-{}", i))
            .build()
            .expect("rayon pool with fixed thread count and default stack cannot fail to build")
    })
}

/// RAII guard that decrements ACTIVE_INDEXING on drop (even on panic).
pub(super) struct IndexGuard;

impl IndexGuard {
    /// Wait for a free indexing slot (at most `MAX_CONCURRENT_INDEXING`
    /// jobs run at once) and hold it until the guard is dropped.
    pub(super) fn acquire() -> Self {
        use std::sync::atomic::Ordering;
        loop {
            let current = ACTIVE_INDEXING.load(Ordering::SeqCst);
            if current < MAX_CONCURRENT_INDEXING
                && ACTIVE_INDEXING
                    .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                return IndexGuard;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
impl Drop for IndexGuard {
    fn drop(&mut self) {
        ACTIVE_INDEXING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
