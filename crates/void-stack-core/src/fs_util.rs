//! Shared file-system helpers used across the crate.

use std::io::{BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Streaming SHA-256 of a file. Returns an empty string if the file cannot
/// be opened or read — callers treat `""` as *"skip hash comparison"*.
///
/// Uses a 64 KB buffer so arbitrarily large files never load fully into RAM.
pub fn file_sha256(path: &Path) -> String {
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
