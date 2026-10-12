//! Helpers for tests that drive a real `git` in throwaway repositories.
//!
//! Not part of the API; public only so the MCP and CLI crates' tests can
//! share it.
//!
//! On Windows, `git` writes `config`, objects and refs to a `*.lock` /
//! temp file and renames it into place. Antivirus and search indexers scan
//! new files under `%TEMP%` and briefly hold them open, so under a busy
//! parallel `cargo test` a rename can fail with "Permission denied" /
//! "failed to write commit object" although nothing is wrong with the
//! test. Those failures leave the repository unchanged, so the command is
//! retried a few times before the test is failed.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// stderr fragments of failures caused by another process holding a file.
const TRANSIENT: &[&str] = &[
    "Permission denied",
    "Access is denied",
    "being used by another process",
    "failed to write",
    "could not write",
    "unable to write",
    "unable to create",
    "unable to unlink",
    "cannot lock",
    "Unable to create",
    "Resource temporarily unavailable",
];

const ATTEMPTS: u32 = 6;

/// Run `git -C <dir> <args>`, retrying transient file-lock failures, and
/// return its stdout. Panics with git's stderr on a real failure.
#[doc(hidden)]
pub fn git(dir: &Path, args: &[&str]) -> String {
    let mut last_err = String::new();
    for attempt in 1..=ATTEMPTS {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git must be installed for this test");
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).into_owned();
        }
        last_err = String::from_utf8_lossy(&out.stderr).into_owned();
        if !TRANSIENT.iter().any(|t| last_err.contains(t)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100 * attempt as u64));
    }
    panic!(
        "git {:?} failed in {}: {}",
        args,
        dir.display(),
        last_err.trim()
    );
}

/// `git init` plus a local identity and no commit signing.
#[doc(hidden)]
pub fn init_repo(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "t@t.io"]);
    git(dir, &["config", "user.name", "t"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_returns_stdout_and_init_repo_configures_identity() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        assert_eq!(git(dir.path(), &["config", "user.name"]).trim(), "t");
    }

    #[test]
    #[should_panic(expected = "failed in")]
    fn git_panics_on_real_failures_without_retrying_forever() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["rev-parse", "HEAD"]);
    }
}
