//! Safe file reader for project files.
//!
//! Reads any file from a registered project by relative path,
//! respecting security constraints (blocks sensitive files) and
//! enforcing a size limit to prevent memory issues.

use std::path::Path;

use crate::error::{Result, VoidStackError};
use crate::security::is_sensitive_file;

/// Maximum file size in bytes (200 KB).
const MAX_FILE_SIZE: u64 = 200 * 1024;

/// Read a file from a project directory by relative path.
///
/// # Security
/// - Blocks path traversal attempts (`..` segments)
/// - Rejects sensitive files (`.env`, credentials, private keys, etc.)
///
/// # Limits
/// - Files larger than 200 KB are truncated with a warning appended.
///
/// # Arguments
/// - `project_path`: absolute path to the project root
/// - `relative_path`: path relative to the project root (e.g. `src/main.rs`)
pub fn read_project_file(project_path: &Path, relative_path: &str) -> Result<String> {
    // Block path traversal
    if relative_path.contains("..")
        || relative_path.starts_with('/')
        || relative_path.starts_with('\\')
    {
        return Err(VoidStackError::InvalidConfig(
            "Path traversal is not allowed".to_string(),
        ));
    }

    let full_path = project_path.join(relative_path);

    // Ensure the resolved path is still inside the project
    let canonical_project =
        std::fs::canonicalize(project_path).unwrap_or(project_path.to_path_buf());
    let canonical_file = std::fs::canonicalize(&full_path).map_err(|e| {
        VoidStackError::InvalidConfig(format!("File not found: {relative_path} ({e})"))
    })?;

    if !canonical_file.starts_with(&canonical_project) {
        return Err(VoidStackError::InvalidConfig(
            "Path traversal is not allowed".to_string(),
        ));
    }

    // Security check
    if is_sensitive_file(&canonical_file) {
        return Err(VoidStackError::InvalidConfig(format!(
            "Access denied: '{}' is a sensitive file",
            relative_path
        )));
    }

    // Check existence and that it's a file
    if !canonical_file.is_file() {
        return Err(VoidStackError::InvalidConfig(format!(
            "Not a file: {relative_path}"
        )));
    }

    // Read with size limit
    let metadata = std::fs::metadata(&canonical_file)
        .map_err(|e| VoidStackError::InvalidConfig(format!("Cannot read file metadata: {e}")))?;

    let file_size = metadata.len();

    if file_size > MAX_FILE_SIZE {
        // Read only the first MAX_FILE_SIZE bytes
        let bytes = std::fs::read(&canonical_file)
            .map_err(|e| VoidStackError::InvalidConfig(format!("Cannot read file: {e}")))?;
        let truncated = String::from_utf8_lossy(&bytes[..MAX_FILE_SIZE as usize]);
        Ok(format!(
            "{}\n\n--- [truncated: file is {} bytes, limit is {} bytes] ---",
            truncated, file_size, MAX_FILE_SIZE
        ))
    } else {
        std::fs::read_to_string(&canonical_file)
            .map_err(|e| VoidStackError::InvalidConfig(format!("Cannot read file: {e}")))
    }
}

/// List all files in a project directory (non-recursive, top-level only).
///
/// Returns relative paths, excluding sensitive files and hidden directories.
pub fn list_project_files(project_path: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_files(project_path, project_path, &mut files, 3);
    files.sort();
    files
}

/// Recursively collect files up to a given depth.
fn collect_files(root: &Path, current: &Path, files: &mut Vec<String>, depth: u32) {
    if depth == 0 {
        return;
    }

    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden dirs, node_modules, target, .git, etc.
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "__pycache__"
            || name == "dist"
            || name == "build"
        {
            continue;
        }

        if path.is_dir() {
            collect_files(root, &path, files, depth - 1);
        } else if path.is_file()
            && !is_sensitive_file(&path)
            && let Ok(rel) = path.strip_prefix(root)
        {
            files.push(rel.to_string_lossy().to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a temp project directory for testing.
    fn setup_test_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");
        // Normal files
        fs::write(dir.path().join("README.md"), "# Hello").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        // Subdirectory
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }",
        )
        .unwrap();
        // Sensitive files
        fs::write(dir.path().join(".env"), "SECRET=123").unwrap();
        fs::write(dir.path().join("credentials.json"), "{}").unwrap();
        fs::write(dir.path().join("id_rsa"), "private-key").unwrap();
        fs::write(dir.path().join("token.json"), "{}").unwrap();
        fs::write(dir.path().join("secrets.toml"), "key = \"val\"").unwrap();
        // Sensitive extension
        fs::write(dir.path().join("cert.pem"), "cert").unwrap();
        fs::write(dir.path().join("server.key"), "key").unwrap();
        dir
    }

    #[test]
    fn test_read_normal_file() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "README.md");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "# Hello");
    }

    #[test]
    fn test_read_nested_file() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "src/lib.rs");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("pub fn add"));
    }

    #[test]
    fn test_read_toml_file() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "Cargo.toml");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("[package]"));
    }

    #[test]
    fn test_blocks_env_file() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), ".env");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("sensitive"),
            "Expected sensitive error, got: {err}"
        );
    }

    #[test]
    fn test_blocks_credentials_json() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "credentials.json");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sensitive"));
    }

    #[test]
    fn test_blocks_private_key() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "id_rsa");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sensitive"));
    }

    #[test]
    fn test_blocks_token_json() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "token.json");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sensitive"));
    }

    #[test]
    fn test_blocks_secrets_toml() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "secrets.toml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sensitive"));
    }

    #[test]
    fn test_blocks_pem_extension() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "cert.pem");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sensitive"));
    }

    #[test]
    fn test_blocks_key_extension() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "server.key");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sensitive"));
    }

    #[test]
    fn test_blocks_path_traversal_dotdot() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }

    #[test]
    fn test_blocks_absolute_path() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "/etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }

    #[test]
    fn test_blocks_backslash_absolute() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "\\Windows\\System32\\config");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }

    #[test]
    fn test_file_not_found() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "nonexistent.txt");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found") || err.contains("File not found"),
            "Got: {err}"
        );
    }

    #[test]
    fn test_directory_is_not_a_file() {
        let dir = setup_test_project();
        let result = read_project_file(dir.path(), "src");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Not a file"));
    }

    #[test]
    fn test_truncates_large_file() {
        let dir = setup_test_project();
        // Create a file larger than 200KB
        let big_content = "x".repeat(250 * 1024);
        fs::write(dir.path().join("big.txt"), &big_content).unwrap();

        let result = read_project_file(dir.path(), "big.txt");
        assert!(result.is_ok());
        let content = result.unwrap();
        assert!(content.contains("[truncated:"));
        assert!(content.len() < big_content.len());
    }

    #[test]
    fn test_list_project_files() {
        let dir = setup_test_project();
        let files = list_project_files(dir.path());
        // Should include normal files
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"Cargo.toml".to_string()));
        assert!(files.contains(&"main.rs".to_string()));
        // Should NOT include sensitive files
        assert!(!files.iter().any(|f| f.contains(".env")));
        assert!(!files.iter().any(|f| f.contains("credentials")));
        assert!(!files.iter().any(|f| f.contains("id_rsa")));
    }

    #[test]
    fn test_list_includes_nested_files() {
        let dir = setup_test_project();
        let files = list_project_files(dir.path());
        let has_lib = files.iter().any(|f| f.contains("lib.rs"));
        assert!(has_lib, "Should include src/lib.rs, got: {:?}", files);
    }

    #[test]
    fn test_read_empty_file() {
        let dir = setup_test_project();
        fs::write(dir.path().join("empty.txt"), "").unwrap();
        let result = read_project_file(dir.path(), "empty.txt");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_read_invalid_utf8_small_file_errors() {
        // A small file with invalid UTF-8 goes through read_to_string, which fails.
        let dir = setup_test_project();
        fs::write(dir.path().join("binary.bin"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
        let result = read_project_file(dir.path(), "binary.bin");
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("Cannot read file"),
            "expected read failure for invalid UTF-8"
        );
    }

    #[test]
    fn test_truncates_large_non_utf8_file() {
        // A large file with invalid UTF-8 exercises the truncation branch, which
        // uses from_utf8_lossy and therefore still succeeds.
        let dir = setup_test_project();
        let big: Vec<u8> = std::iter::repeat_n(0xff_u8, 250 * 1024).collect();
        fs::write(dir.path().join("big.bin"), &big).unwrap();
        let result = read_project_file(dir.path(), "big.bin");
        assert!(result.is_ok());
        let content = result.unwrap();
        assert!(content.contains("[truncated:"));
        // 250 KiB = 256000 bytes; the marker reports the real size.
        assert!(content.contains("256000"));
        // Invalid bytes are lossily replaced, so the result is valid UTF-8.
        assert!(content.contains('\u{FFFD}'));
    }

    #[test]
    fn test_list_excludes_skip_directories() {
        let dir = setup_test_project();
        // Files buried in skip directories must never be listed.
        for skip in ["node_modules", "target", "dist", "build", "__pycache__"] {
            let sub = dir.path().join(skip);
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join("inside.txt"), "data").unwrap();
        }
        let files = list_project_files(dir.path());
        assert!(
            !files.iter().any(|f| f.contains("inside.txt")),
            "skip directories should be excluded, got: {:?}",
            files
        );
    }

    #[test]
    fn test_list_excludes_hidden_directories() {
        let dir = setup_test_project();
        let git = dir.path().join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::write(git.join("config"), "[core]").unwrap();
        let files = list_project_files(dir.path());
        assert!(
            !files.iter().any(|f| f.contains(".git")),
            "hidden directories should be excluded, got: {:?}",
            files
        );
    }

    #[test]
    fn test_list_respects_depth_limit() {
        let dir = setup_test_project();
        // Nesting beyond the collector's depth (3) is not traversed.
        let deep = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("too_deep.txt"), "x").unwrap();
        // A file within the allowed depth should still be listed.
        fs::write(dir.path().join("a").join("shallow.txt"), "y").unwrap();
        let files = list_project_files(dir.path());
        assert!(
            files.iter().any(|f| f.contains("shallow.txt")),
            "shallow file should be listed, got: {:?}",
            files
        );
        assert!(
            !files.iter().any(|f| f.contains("too_deep.txt")),
            "file beyond depth limit should be excluded, got: {:?}",
            files
        );
    }
}
