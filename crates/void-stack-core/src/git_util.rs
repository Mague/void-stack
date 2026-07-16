//! WSL-aware git invocation shared by the git-backed features (board
//! history, work timeline).
//!
//! Two problems solved here:
//! - **Windows process-spawn cost.** Creating a process on Windows is an
//!   order of magnitude slower than on Unix, so anything that used to
//!   spawn one `git show` per commit now reads every object through a
//!   single `git cat-file --batch` process ([`batch_read_objects`]).
//! - **WSL project roots.** Windows git cannot operate on a repo behind a
//!   `\\wsl.localhost\…` / `\\wsl$\…` UNC root (ownership checks fail and
//!   the working directory can't be set), which silently degraded the
//!   board history to empty. Commands against a WSL root are routed
//!   through `wsl.exe -d <distro> git -C <linux-path>` instead.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::process_util::HideWindow;
use crate::runner::local::{is_wsl_unc_path, unc_to_linux_path, unc_to_wsl_distro};

/// Program and leading args that make `git <args…>` run against `root`:
/// plain `git -C <root>` for native paths, `wsl.exe … git -C <linux>` for
/// WSL UNC roots. Split out from [`git_command`] so it stays unit-testable
/// without spawning anything.
fn git_invocation(root: &Path) -> (String, Vec<String>) {
    let root_str = root.to_string_lossy().to_string();
    if is_wsl_unc_path(&root_str) {
        let mut args = Vec::new();
        if let Some(distro) = unc_to_wsl_distro(&root_str) {
            args.push("-d".to_string());
            args.push(distro);
        }
        args.extend(
            ["--", "git", "-C"]
                .into_iter()
                .map(str::to_string)
                .chain([unc_to_linux_path(&root_str)]),
        );
        ("wsl.exe".to_string(), args)
    } else {
        ("git".to_string(), vec!["-C".to_string(), root_str])
    }
}

fn git_command(root: &Path) -> Command {
    let (program, prefix) = git_invocation(root);
    let mut cmd = Command::new(program);
    cmd.args(prefix);
    cmd.hide_window();
    cmd
}

/// Run one git command against `root` and return stdout (lossy UTF-8).
pub fn git_output(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_command(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git {:?}: {}", args, e))?;
    if !out.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Read many objects (`<rev>:<path>` specs) through ONE `git cat-file
/// --batch` process. Returns one entry per spec, in input order; `None`
/// when the object doesn't exist in that revision.
///
/// This replaces per-commit `git show` spawns: for a board with N
/// committed revisions the old path cost 1 + N..2N process launches,
/// this one costs exactly 2 regardless of N.
pub fn batch_read_objects(root: &Path, specs: &[String]) -> Result<Vec<Option<Vec<u8>>>, String> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let mut child = git_command(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("git cat-file --batch: {}", e))?;

    // Feed requests from a separate thread: if git fills the stdout pipe
    // while we are still writing, a single-threaded write-then-read would
    // deadlock.
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input = specs.join("\n") + "\n";
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
        // Dropping stdin closes the pipe and ends the batch session.
    });

    let stdout = child.stdout.take().expect("stdout is piped");
    let result = read_batch_responses(BufReader::new(stdout), specs.len());
    let _ = writer.join();
    let _ = child.wait();
    result
}

/// Parse `cat-file --batch` responses: hits are `<oid> <type> <size>\n`
/// followed by `<size>` bytes and a trailing newline; misses echo the
/// spec plus ` missing` (or ` ambiguous`) and carry no payload.
fn read_batch_responses<R: BufRead>(
    mut reader: R,
    expected: usize,
) -> Result<Vec<Option<Vec<u8>>>, String> {
    let mut results = Vec::with_capacity(expected);
    for _ in 0..expected {
        let mut header = String::new();
        let n = reader
            .read_line(&mut header)
            .map_err(|e| format!("git cat-file --batch read: {}", e))?;
        if n == 0 {
            // Early EOF (killed process, corrupt repo): remaining specs
            // are unanswerable, not an error worth failing the whole
            // history for.
            results.push(None);
            continue;
        }
        let fields: Vec<&str> = header.split_whitespace().collect();
        let size = match fields.as_slice() {
            [_oid, _type, size] => size.parse::<usize>().ok(),
            _ => None,
        };
        let Some(size) = size else {
            results.push(None);
            continue;
        };
        let mut buf = vec![0u8; size + 1]; // payload + trailing '\n'
        reader
            .read_exact(&mut buf)
            .map_err(|e| format!("git cat-file --batch payload: {}", e))?;
        buf.pop();
        results.push(Some(buf));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sh_git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            st.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&st.stderr)
        );
    }

    fn repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        sh_git(&root, &["init", "-q"]);
        sh_git(&root, &["config", "user.email", "t@t.io"]);
        sh_git(&root, &["config", "user.name", "t"]);
        sh_git(&root, &["config", "commit.gpgsign", "false"]);
        (tmp, root)
    }

    fn commit_file(root: &Path, file: &str, content: &str, msg: &str) -> String {
        std::fs::write(root.join(file), content).unwrap();
        sh_git(root, &["add", "."]);
        sh_git(root, &["commit", "-q", "-m", msg]);
        let out = Command::new("git")
            .args([
                "-C",
                &root.to_string_lossy(),
                "rev-parse",
                "--short",
                "HEAD",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn test_git_invocation_native_path() {
        let (program, args) = git_invocation(Path::new(r"F:\workspace\project"));
        assert_eq!(program, "git");
        assert_eq!(
            args,
            vec!["-C".to_string(), r"F:\workspace\project".to_string()]
        );
    }

    #[test]
    fn test_git_invocation_wsl_unc_path_routes_through_wsl() {
        let (program, args) =
            git_invocation(Path::new(r"\\wsl.localhost\Ubuntu\home\user\project"));
        assert_eq!(program, "wsl.exe");
        assert_eq!(
            args,
            vec!["-d", "Ubuntu", "--", "git", "-C", "/home/user/project"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_git_invocation_wsl_dollar_prefix() {
        let (program, args) = git_invocation(Path::new(r"\\wsl$\Debian\opt\app"));
        assert_eq!(program, "wsl.exe");
        assert_eq!(args[1], "Debian");
        assert_eq!(args.last().unwrap(), "/opt/app");
    }

    #[test]
    fn test_git_output_runs_and_fails_cleanly() {
        let (_tmp, root) = repo();
        commit_file(&root, "a.txt", "hello", "first");
        let log = git_output(&root, &["log", "--format=%s"]).unwrap();
        assert_eq!(log.trim(), "first");
        // Bad revision → Err with git's stderr, not a panic.
        assert!(git_output(&root, &["show", "nope:missing.txt"]).is_err());
    }

    #[test]
    fn test_batch_read_objects_hits_and_misses_in_order() {
        let (_tmp, root) = repo();
        let h1 = commit_file(&root, "a.txt", "v1", "c1");
        let h2 = commit_file(&root, "a.txt", "v2", "c2");

        let specs = vec![
            format!("{}:a.txt", h1),
            format!("{}:nope.txt", h1), // missing in c1
            format!("{}:a.txt", h2),
        ];
        let objs = batch_read_objects(&root, &specs).unwrap();
        assert_eq!(objs.len(), 3);
        assert_eq!(objs[0].as_deref(), Some(b"v1".as_slice()));
        assert_eq!(objs[1], None);
        assert_eq!(objs[2].as_deref(), Some(b"v2".as_slice()));
    }

    #[test]
    fn test_batch_read_objects_empty_specs_spawns_nothing() {
        // Must not touch git at all — works even outside a repo.
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(batch_read_objects(tmp.path(), &[]).unwrap(), Vec::new());
    }

    #[test]
    fn test_read_batch_responses_parses_mixed_stream() {
        // Simulated cat-file output: one hit, one miss, then early EOF.
        let stream = b"0123456 blob 5\nhello\nabc:missing.txt missing\n";
        let out = read_batch_responses(&stream[..], 3).unwrap();
        assert_eq!(out[0].as_deref(), Some(b"hello".as_slice()));
        assert_eq!(out[1], None);
        assert_eq!(out[2], None); // EOF degrades to missing, never hangs
    }
}
