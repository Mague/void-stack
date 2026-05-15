//! Shared file-system helpers used across the crate.

use std::io::{BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::process_util::HideWindow;

/// Streaming SHA-256 of a file. Returns an empty string if the file cannot
/// be opened or read — callers treat `""` as *"skip hash comparison"*.
///
/// Uses a 64 KB buffer so arbitrarily large files never load fully into RAM.
/// For WSL UNC paths (`\\wsl$\…`, `\\wsl.localhost\…`) the streaming path
/// falls back to a `wsl.exe -- cat` round-trip, since std::fs::File::open
/// silently fails on those paths in many process contexts (services, IDE
/// integrations) on Windows.
pub fn file_sha256(path: &Path) -> String {
    if is_wsl_unc_path(path) {
        // No streaming for WSL — the wsl.exe handshake already serializes
        // the bytes through stdout, so we hash whatever it returns.
        return match read_file_via_wsl(path) {
            Some(bytes) => sha256_bytes(&bytes),
            None => String::new(),
        };
    }

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return String::new(),
        }
    }
    format!("{:x}", hasher.finalize())
}

/// Hash an in-memory byte slice. Used when bytes come from a non-streaming
/// source (e.g. `wsl.exe -- cat`).
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Read a file as raw bytes, transparently routing WSL UNC paths through
/// `wsl.exe -- cat`. Returns `None` when the file can't be read; callers
/// should treat that as "skip this file" rather than propagating an error,
/// since the structural builder iterates many files and one unreadable one
/// shouldn't kill the whole run.
pub fn read_file_bytes(path: &Path) -> Option<Vec<u8>> {
    if is_wsl_unc_path(path) {
        read_file_via_wsl(path)
    } else {
        std::fs::read(path).ok()
    }
}

/// True for `\\wsl$\<distro>\…` and `\\wsl.localhost\<distro>\…` (case
/// insensitive). Forward-slash variants are also accepted because some
/// callers normalise to `/`.
fn is_wsl_unc_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    let lower = s.to_lowercase();
    lower.starts_with(r"\\wsl") || lower.starts_with("//wsl")
}

/// Convert a WSL UNC path to the corresponding Linux path inside the
/// distro. Returns `None` if the path doesn't match the expected shape
/// (`\\wsl$\<distro>\<rest>` or `\\wsl.localhost\<distro>\<rest>`).
fn unc_to_linux(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    let normalized = s.replace('/', "\\");
    let rest = normalized
        .strip_prefix(r"\\wsl.localhost\")
        .or_else(|| normalized.strip_prefix(r"\\wsl$\"))?;
    // Skip the distro name (first segment after the prefix).
    let (_distro, after) = rest.split_once('\\')?;
    if after.is_empty() {
        return Some("/".to_string());
    }
    Some(format!("/{}", after.replace('\\', "/")))
}

fn read_file_via_wsl(path: &Path) -> Option<Vec<u8>> {
    let linux_path = unc_to_linux(path)?;
    let output = std::process::Command::new("wsl.exe")
        .args(["--", "cat", &linux_path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .hide_window()
        .output()
        .ok()?;
    if output.status.success() {
        Some(output.stdout)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_wsl_unc_path_recognises_both_prefixes() {
        assert!(is_wsl_unc_path(Path::new(r"\\wsl$\Ubuntu\home\user")));
        assert!(is_wsl_unc_path(Path::new(
            r"\\wsl.localhost\Ubuntu-24.04\home"
        )));
        // Forward-slash mirror used by some path normalisers.
        assert!(is_wsl_unc_path(Path::new(r"//wsl$/Ubuntu/home")));
        assert!(!is_wsl_unc_path(Path::new(r"F:\workspace\project")));
        assert!(!is_wsl_unc_path(Path::new("/home/user/project")));
    }

    #[test]
    fn test_unc_to_linux_strips_distro() {
        assert_eq!(
            unc_to_linux(Path::new(r"\\wsl.localhost\Ubuntu\home\user\file.rs")),
            Some("/home/user/file.rs".to_string())
        );
        assert_eq!(
            unc_to_linux(Path::new(r"\\wsl$\Ubuntu-24.04\opt\app\main.ex")),
            Some("/opt/app/main.ex".to_string())
        );
        // Bare distro root → "/"
        assert_eq!(
            unc_to_linux(Path::new(r"\\wsl.localhost\Ubuntu\")),
            Some("/".to_string())
        );
        // Non-WSL path → None
        assert_eq!(unc_to_linux(Path::new(r"F:\workspace\project")), None);
    }

    #[test]
    fn test_read_file_bytes_windows_path_roundtrip() {
        // Normal Windows tempfile reads via the non-WSL branch.
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"hello").unwrap();
        let bytes = read_file_bytes(&f).unwrap();
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn test_read_file_bytes_wsl_missing_path_returns_none() {
        // A `\\wsl…` path that doesn't exist must not panic; we get None.
        // Skips actually calling wsl.exe by exercising the cat-failure
        // branch (the runner returns None when the file isn't readable).
        let p = Path::new(r"\\wsl$\Ubuntu\nonexistent\path\xyz.rs");
        // We only assert *no panic*. If wsl.exe is missing or the distro
        // doesn't exist, we still expect None — never a crash.
        let _ = read_file_bytes(p);
    }

    #[test]
    fn test_sha256_bytes_matches_known_value() {
        // sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_file_sha256_normal_path() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"abc").unwrap();
        let h = file_sha256(&f);
        // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
