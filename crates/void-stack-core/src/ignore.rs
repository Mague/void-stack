//! `.voidignore` support for excluding files from analysis.
//!
//! Pattern matching rules (simplified `.gitignore`-style):
//! - Lines starting with `#` are comments
//! - Empty lines are ignored
//! - `path/to/dir/` — ignores any path starting with this prefix
//! - `**/*.ext` — ignores any file ending with `.ext`
//! - `dirname/` — ignores an exact top-level directory

use std::path::Path;

/// Holds parsed patterns from a `.voidignore` file.
pub struct VoidIgnore {
    patterns: Vec<Pattern>,
    raw_lines: Vec<String>,
}

enum Pattern {
    /// `internal/pb/` → path starts with prefix
    Prefix(String),
    /// `**/*.pb.go` → path ends with suffix
    Suffix(String),
    /// Exact directory name match at any level (e.g. `vendor/` → any segment == "vendor")
    DirName(String),
}

impl VoidIgnore {
    /// Load `.claudeignore` from the project root.
    /// If the file doesn't exist, returns an empty instance (nothing ignored).
    pub fn load_claudeignore(project_root: &Path) -> Self {
        let path = project_root.join(".claudeignore");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => {
                return Self {
                    patterns: Vec::new(),
                    raw_lines: Vec::new(),
                };
            }
        };
        Self::from_content(&content)
    }

    /// Load `.voidignore` from the project root.
    /// If the file doesn't exist, returns an empty instance (nothing ignored).
    pub fn load(project_root: &Path) -> Self {
        let path = project_root.join(".voidignore");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => {
                return Self {
                    patterns: Vec::new(),
                    raw_lines: Vec::new(),
                };
            }
        };

        let mut patterns = Vec::new();
        let mut raw_lines = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            raw_lines.push(trimmed.to_string());

            let normalized = trimmed.replace('\\', "/");

            if let Some(after_glob) = normalized.strip_prefix("**/") {
                // **/*.pb.go → match files ending with .pb.go
                // **/mocks/ → match directory at any level
                if after_glob.ends_with('/') {
                    // Directory glob: treat as DirName
                    let name = after_glob.trim_end_matches('/').to_string();
                    patterns.push(Pattern::DirName(name));
                } else if let Some(ext) = after_glob.strip_prefix('*') {
                    // **/*.pb.go → suffix on the filename extension part
                    patterns.push(Pattern::Suffix(ext.to_string()));
                } else {
                    // **/something → prefix match with the suffix
                    patterns.push(Pattern::Suffix(after_glob.to_string()));
                }
            } else if normalized.ends_with('/') {
                if !normalized[..normalized.len() - 1].contains('/') {
                    // Single dir name like `vendor/` → match at any level
                    let name = normalized.trim_end_matches('/').to_string();
                    patterns.push(Pattern::DirName(name));
                } else {
                    // `internal/pb/` → prefix match
                    patterns.push(Pattern::Prefix(normalized));
                }
            } else {
                // Treat as prefix (e.g. `internal/pb` without trailing slash)
                patterns.push(Pattern::Prefix(normalized));
            }
        }

        Self {
            patterns,
            raw_lines,
        }
    }

    /// Returns `true` if the given relative path should be ignored.
    pub fn is_ignored(&self, relative_path: &str) -> bool {
        if self.patterns.is_empty() {
            return false;
        }

        let normalized = relative_path.replace('\\', "/");

        for pattern in &self.patterns {
            match pattern {
                Pattern::Prefix(prefix) => {
                    if normalized.starts_with(prefix.as_str()) {
                        return true;
                    }
                }
                Pattern::Suffix(suffix) => {
                    if normalized.ends_with(suffix.as_str()) {
                        return true;
                    }
                }
                Pattern::DirName(name) => {
                    // Match: starts with "name/", contains "/name/", or equals "name"
                    if normalized.starts_with(&format!("{}/", name))
                        || normalized.contains(&format!("/{}/", name))
                        || normalized == *name
                    {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Returns `true` if a `.voidignore` file was loaded with patterns.
    pub fn is_active(&self) -> bool {
        !self.patterns.is_empty()
    }

    /// Parse patterns from a string (without reading from disk).
    pub fn from_content(content: &str) -> Self {
        let mut patterns = Vec::new();
        let mut raw_lines = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            raw_lines.push(trimmed.to_string());
            let normalized = trimmed.replace('\\', "/");

            if let Some(after_glob) = normalized.strip_prefix("**/") {
                if after_glob.ends_with('/') {
                    let name = after_glob.trim_end_matches('/').to_string();
                    patterns.push(Pattern::DirName(name));
                } else if let Some(ext) = after_glob.strip_prefix('*') {
                    patterns.push(Pattern::Suffix(ext.to_string()));
                } else {
                    patterns.push(Pattern::Suffix(after_glob.to_string()));
                }
            } else if normalized.ends_with('/') {
                if !normalized[..normalized.len() - 1].contains('/') {
                    let name = normalized.trim_end_matches('/').to_string();
                    patterns.push(Pattern::DirName(name));
                } else {
                    patterns.push(Pattern::Prefix(normalized));
                }
            } else {
                patterns.push(Pattern::Prefix(normalized));
            }
        }

        Self {
            patterns,
            raw_lines,
        }
    }

    /// Returns the raw pattern lines for display purposes.
    pub fn pattern_lines(&self) -> &[String] {
        &self.raw_lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_ignores_pb_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "internal/pb/\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("internal/pb/foo.go"));
        assert!(ignore.is_ignored("internal/pb/bar.pb.go"));
        assert!(!ignore.is_ignored("internal/services/foo.go"));
    }

    #[test]
    fn test_ignores_pb_go_extension() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "**/*.pb.go\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("internal/pb/foo.pb.go"));
        assert!(ignore.is_ignored("any/deep/path/bar.pb.go"));
        assert!(!ignore.is_ignored("internal/services/foo.go"));
    }

    #[test]
    fn test_ignores_pb_gw_go_extension() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "**/*.pb.gw.go\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("api/v1/service.pb.gw.go"));
        assert!(!ignore.is_ignored("api/v1/service.pb.go"));
    }

    #[test]
    fn test_not_ignores_regular_go() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "internal/pb/\n**/*.pb.go\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(!ignore.is_ignored("internal/services/foo.go"));
        assert!(!ignore.is_ignored("cmd/main.go"));
    }

    #[test]
    fn test_empty_voidignore() {
        let dir = tempfile::tempdir().unwrap();
        // No .voidignore file
        let ignore = VoidIgnore::load(dir.path());
        assert!(!ignore.is_active());
        assert!(!ignore.is_ignored("anything/at/all.go"));
    }

    #[test]
    fn test_comments_skipped() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".voidignore"),
            "# This is a comment\n\n# Another comment\ninternal/pb/\n",
        )
        .unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert_eq!(ignore.pattern_lines().len(), 1);
        assert!(ignore.is_ignored("internal/pb/foo.go"));
    }

    #[test]
    fn test_vendor_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "vendor/\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("vendor/github.com/pkg/foo.go"));
        assert!(ignore.is_ignored("some/nested/vendor/lib.go"));
        assert!(!ignore.is_ignored("vendors/something.go")); // no match — "vendors" != "vendor"
    }

    #[test]
    fn test_generated_directory_glob() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "**/generated/\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("generated/types.go"));
        assert!(ignore.is_ignored("internal/generated/models.go"));
    }

    #[test]
    fn test_mocks_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "**/mocks/\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("mocks/service_mock.go"));
        assert!(ignore.is_ignored("internal/mocks/repo_mock.go"));
    }

    #[test]
    fn test_mock_go_suffix() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "**/*_mock.go\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("internal/repo_mock.go"));
        assert!(!ignore.is_ignored("internal/repo.go"));
    }

    #[test]
    fn test_multiple_patterns() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".voidignore"),
            "# Generated code\ninternal/pb/\nvendor/\n**/*.pb.go\n**/*.pb.gw.go\n\n# Mocks\n**/mocks/\n**/*_mock.go\n",
        )
        .unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_active());
        assert_eq!(ignore.pattern_lines().len(), 6);

        assert!(ignore.is_ignored("internal/pb/user.pb.go"));
        assert!(ignore.is_ignored("vendor/github.com/lib/pq/conn.go"));
        assert!(ignore.is_ignored("api/v1/service.pb.gw.go"));
        assert!(ignore.is_ignored("mocks/user_mock.go"));
        assert!(!ignore.is_ignored("internal/services/user.go"));
        assert!(!ignore.is_ignored("cmd/server/main.go"));
    }

    #[test]
    fn test_windows_backslash_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "internal/pb/\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("internal\\pb\\foo.go"));
    }

    #[test]
    fn test_is_active() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "internal/pb/\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_active());
    }

    #[test]
    fn test_empty_file_not_active() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "# just comments\n\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(!ignore.is_active());
    }

    // Language-agnostic tests
    #[test]
    fn test_ignores_python_generated() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".voidignore"),
            "**/*_pb2.py\n**/*_pb2_grpc.py\n",
        )
        .unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("proto/user_pb2.py"));
        assert!(ignore.is_ignored("proto/user_pb2_grpc.py"));
        assert!(!ignore.is_ignored("services/user.py"));
    }

    #[test]
    fn test_ignores_js_generated() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".voidignore"), "**/*.generated.ts\n").unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("src/graphql/types.generated.ts"));
        assert!(!ignore.is_ignored("src/components/App.tsx"));
    }

    #[test]
    fn test_ignores_rust_generated() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".voidignore"),
            "src/generated/\n**/*.pb.rs\n",
        )
        .unwrap();
        let ignore = VoidIgnore::load(dir.path());
        assert!(ignore.is_ignored("src/generated/api.rs"));
        assert!(ignore.is_ignored("proto/service.pb.rs"));
        assert!(!ignore.is_ignored("src/main.rs"));
    }
}
