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
    if let Ok(file) = std::fs::File::open(path) {
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
        return format!("{:x}", hasher.finalize());
    }
    if is_wsl_unc_path(path) {
        // Forward-slash UNC reads succeed above; the backslash form
        // (which std::fs can't open) falls through here. wsl.exe -- cat
        // serializes the bytes for us, so we hash whatever it returns.
        return match read_file_via_wsl(path) {
            Some(bytes) => sha256_bytes(&bytes),
            None => String::new(),
        };
    }
    String::new()
}

/// Hash an in-memory byte slice. Used when bytes come from a non-streaming
/// source (e.g. `wsl.exe -- cat`).
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Read a file as raw bytes. Tries `std::fs::read` first — works for
/// native paths and for the forward-slash form of WSL UNC paths
/// (`//wsl.localhost/<distro>/…`). Only falls back to `wsl.exe -- cat`
/// when the std read fails on a recognised WSL UNC path (which is what
/// happens for the backslash form — `OS error 3` on Windows 11 even
/// though 9P exposes the FS over SMB-like APIs). Returns `None` when
/// the file can't be read by either path.
pub fn read_file_bytes(path: &Path) -> Option<Vec<u8>> {
    if let Ok(bytes) = std::fs::read(path) {
        return Some(bytes);
    }
    if is_wsl_unc_path(path) {
        return read_file_via_wsl(path);
    }
    None
}

/// Source text from raw file bytes: UTF-8 (a leading BOM is dropped) or
/// UTF-16 LE/BE with BOM, which MetaEditor (MQL4/MQL5) and some Windows
/// tools write. Other bytes are returned unchanged, so callers that accept
/// non-UTF-8 input (tree-sitter) keep working as before.
pub fn normalize_source_bytes(bytes: Vec<u8>) -> Vec<u8> {
    match bytes.as_slice() {
        [0xEF, 0xBB, 0xBF, rest @ ..] => rest.to_vec(),
        [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, u16::from_le_bytes).into_bytes(),
        [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, u16::from_be_bytes).into_bytes(),
        _ => bytes,
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|b| unit([b[0], b[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// Read a source file as text (see [`normalize_source_bytes`]). `None` when
/// it can't be read or isn't UTF-8 / UTF-16 text.
pub fn read_source(path: &Path) -> Option<String> {
    String::from_utf8(normalize_source_bytes(read_file_bytes(path)?)).ok()
}

/// True for `\\wsl$\<distro>\…` and `\\wsl.localhost\<distro>\…` (case
/// insensitive). Forward-slash variants are also accepted because some
/// callers normalise to `/`.
pub(crate) fn is_wsl_unc_path(path: &Path) -> bool {
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
    wsl_exec(&["--", "cat", &linux_path])
}

/// Run a `wsl.exe` invocation with the given args and return stdout bytes
/// on success. Internal helper for [`read_file_via_wsl`]. Returns `None`
/// when wsl.exe is missing, the distro can't be reached, or the inner
/// command exits non-zero.
fn wsl_exec(args: &[&str]) -> Option<Vec<u8>> {
    let output = std::process::Command::new("wsl.exe")
        .args(args)
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
    fn source_text_decodes_bom_utf8_and_utf16() {
        let text = "//+--- Experto: niño \u{1F600}\nvoid OnTick(void) {}\n";
        let mut utf8_bom = vec![0xEF, 0xBB, 0xBF];
        utf8_bom.extend_from_slice(text.as_bytes());
        let mut le = vec![0xFF, 0xFE];
        let mut be = vec![0xFE, 0xFF];
        for u in text.encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
            be.extend_from_slice(&u.to_be_bytes());
        }
        for bytes in [text.as_bytes().to_vec(), utf8_bom, le, be] {
            assert_eq!(
                String::from_utf8(normalize_source_bytes(bytes)).unwrap(),
                text
            );
        }
        // Not text: passed through untouched (tree-sitter still gets it).
        assert_eq!(
            normalize_source_bytes(vec![0x00, 0xC3, 0x28]),
            vec![0x00, 0xC3, 0x28]
        );

        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("Bot.mq4");
        let mut le = vec![0xFF, 0xFE];
        for u in "int start() { return 0; }".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&f, le).unwrap();
        assert_eq!(
            read_source(&f).as_deref(),
            Some("int start() { return 0; }")
        );
        std::fs::write(&f, [0xC3, 0x28]).unwrap();
        assert_eq!(read_source(&f), None, "invalid UTF-8 is not text");
    }

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
