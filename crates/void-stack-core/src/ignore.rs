//! `.voidignore` support for excluding files from analysis.
//!
//! Pattern matching rules (simplified `.gitignore`-style):
//! - Lines starting with `#` are comments
//! - Empty lines are ignored
//! - `path/to/dir/` — ignores any path starting with this prefix
//! - `**/*.ext` or `*.ext` — ignores any file ending with `.ext`
//! - `dirname/` — ignores a directory with that name at any level
//! - `prefix*/` — ignores any directory whose name starts with `prefix`
//!   (e.g. CLion's `cmake-build-*/`)

use std::path::Path;

/// Holds parsed patterns from a `.voidignore` file.
pub struct VoidIgnore {
    patterns: Vec<Pattern>,
    raw_lines: Vec<String>,
    /// The root is a Unity or Godot project: their build/cache folders
    /// (`Library/`, `Temp/`, `obj/`, …) are skipped. Those names are too
    /// generic to skip in every project, so this is decided per root.
    engine_build_dirs: bool,
}

/// Generated folders skipped in every project: names that only tools
/// create (Godot's `.godot`/`.import`, SwiftPM's `.build`/`.swiftpm`,
/// Xcode's `DerivedData`/`xcuserdata`, CocoaPods, Carthage).
const TOOL_GENERATED_DIRS: &[&str] = &[
    ".godot",
    ".import",
    ".build",
    ".swiftpm",
    "DerivedData",
    "xcuserdata",
    "Pods",
    "Carthage",
    // Visual Studio's per-user state and vcpkg's manifest-mode install tree.
    ".vs",
    "vcpkg_installed",
];

/// Build/cache folders of Unity and Godot (C#) projects — skipped only
/// when the root carries the engine's markers.
const ENGINE_BUILD_DIRS: &[&str] = &[
    "Library",
    "Temp",
    "Obj",
    "obj",
    "Logs",
    "UserSettings",
    "MemoryCaptures",
    "Build",
    "Builds",
    ".mono",
];

fn root_has_engine_build_dirs(root: &Path) -> bool {
    crate::config::has_unity_markers(root) || crate::config::has_godot_markers(root)
}

enum Pattern {
    /// `internal/pb/` → path starts with prefix
    Prefix(String),
    /// `**/*.pb.go` → path ends with suffix
    Suffix(String),
    /// Exact directory name match at any level (e.g. `vendor/` → any segment == "vendor")
    DirName(String),
    /// Directory name prefix at any level (`cmake-build-*/` → any segment
    /// starting with "cmake-build-")
    DirPrefix(String),
}

/// Parse one normalized (forward-slash) pattern line.
fn parse_pattern(normalized: &str) -> Pattern {
    if let Some(after_glob) = normalized.strip_prefix("**/") {
        // **/*.pb.go → match files ending with .pb.go
        // **/mocks/ → match directory at any level
        if after_glob.ends_with('/') {
            Pattern::DirName(after_glob.trim_end_matches('/').to_string())
        } else if let Some(ext) = after_glob.strip_prefix('*') {
            Pattern::Suffix(ext.to_string())
        } else {
            Pattern::Suffix(after_glob.to_string())
        }
    } else if let Some(ext) = normalized
        .strip_prefix('*')
        .filter(|rest| !rest.contains('/') && !rest.contains('*'))
    {
        // `*.obj` (no slash) matches at any level, as in .gitignore.
        Pattern::Suffix(ext.to_string())
    } else if let Some(dir) = normalized.strip_suffix('/') {
        if let Some(prefix) = dir
            .strip_suffix('*')
            .filter(|p| !p.is_empty() && !p.contains('/') && !p.contains('*'))
        {
            Pattern::DirPrefix(prefix.to_string())
        } else if !dir.contains('/') {
            // Single dir name like `vendor/` → match at any level
            Pattern::DirName(dir.to_string())
        } else {
            // `internal/pb/` → prefix match
            Pattern::Prefix(normalized.to_string())
        }
    } else {
        // Treat as prefix (e.g. `internal/pb` without trailing slash)
        Pattern::Prefix(normalized.to_string())
    }
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
                    engine_build_dirs: root_has_engine_build_dirs(project_root),
                };
            }
        };
        let mut loaded = Self::from_content(&content);
        loaded.engine_build_dirs = root_has_engine_build_dirs(project_root);
        loaded
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
                    engine_build_dirs: root_has_engine_build_dirs(project_root),
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
            patterns.push(parse_pattern(&trimmed.replace('\\', "/")));
        }

        Self {
            patterns,
            raw_lines,
            engine_build_dirs: root_has_engine_build_dirs(project_root),
        }
    }

    /// Whether a directory with this name is generated by a game engine or
    /// Apple tooling and should not be walked (see [`TOOL_GENERATED_DIRS`]
    /// and, for Unity/Godot roots, [`ENGINE_BUILD_DIRS`]).
    pub fn is_generated_dir(&self, name: &str) -> bool {
        TOOL_GENERATED_DIRS.contains(&name)
            || (self.engine_build_dirs && ENGINE_BUILD_DIRS.contains(&name))
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
                Pattern::DirPrefix(prefix) => {
                    if normalized
                        .split('/')
                        .any(|seg| seg.starts_with(prefix.as_str()))
                    {
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
            patterns.push(parse_pattern(&trimmed.replace('\\', "/")));
        }

        Self {
            patterns,
            raw_lines,
            engine_build_dirs: false,
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
    fn test_star_extension_matches_at_any_level() {
        let ig = VoidIgnore::from_content("*.obj\n*.pdb\n");
        assert!(ig.is_ignored("main.obj"));
        assert!(ig.is_ignored("build/Release/main.obj"));
        assert!(ig.is_ignored("x64\\Debug\\app.pdb"));
        assert!(!ig.is_ignored("src/main.cpp"));
        assert!(!ig.is_ignored("objects/readme.md"));
    }

    #[test]
    fn test_dir_prefix_glob() {
        let ig = VoidIgnore::from_content("cmake-build-*/\n");
        assert!(ig.is_ignored("cmake-build-debug/"));
        assert!(ig.is_ignored("cmake-build-release/CMakeCache.txt"));
        assert!(ig.is_ignored("sub/cmake-build-debug/app.exe"));
        assert!(!ig.is_ignored("cmake/toolchain.cmake"));
        assert!(!ig.is_ignored("src/build.cpp"));
    }

    #[test]
    fn test_visual_studio_and_vcpkg_dirs_are_generated() {
        let ig = VoidIgnore::from_content("");
        assert!(ig.is_generated_dir(".vs"));
        assert!(ig.is_generated_dir("vcpkg_installed"));
        assert!(!ig.is_generated_dir("src"));
    }

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

    #[test]
    fn test_generated_dirs_skip_engine_folders_only_for_engine_roots() {
        let plain = tempfile::tempdir().unwrap();
        let ig = VoidIgnore::load(plain.path());
        // Tool-only names are always generated.
        assert!(ig.is_generated_dir(".godot"));
        assert!(ig.is_generated_dir("DerivedData"));
        assert!(ig.is_generated_dir(".build"));
        // Generic names are real code in an ordinary project.
        assert!(!ig.is_generated_dir("Library"));
        assert!(!ig.is_generated_dir("Temp"));
        assert!(!ig.is_generated_dir("obj"));

        let unity = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(unity.path().join("Assets")).unwrap();
        std::fs::create_dir_all(unity.path().join("ProjectSettings")).unwrap();
        let ig = VoidIgnore::load(unity.path());
        assert!(ig.is_generated_dir("Library"));
        assert!(ig.is_generated_dir("Temp"));
        assert!(ig.is_generated_dir("obj"));
        assert!(!ig.is_generated_dir("Assets"), "user code stays");

        let godot = tempfile::tempdir().unwrap();
        std::fs::write(godot.path().join("project.godot"), "").unwrap();
        assert!(VoidIgnore::load_claudeignore(godot.path()).is_generated_dir(".mono"));
        assert!(!VoidIgnore::from_content("vendor/\n").is_generated_dir("Library"));
    }
}
