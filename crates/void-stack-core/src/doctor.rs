//! `void doctor`: sanity checks for the global project registry.
//!
//! Detects (and can fix) registry drift that accumulates over time:
//! duplicate registrations of the same directory, projects registered
//! inside other projects, paths that no longer exist, services with a
//! broken working_dir, semantic indexes orphaned by removed projects, and
//! indexes/graphs stale for more than a week. Read-only by default —
//! fixes are explicit `DoctorFix` values a caller applies one by one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::global_config::GlobalConfig;
use crate::runner::local::strip_win_prefix;

/// Indexes/graphs older than this are reported as stale.
pub const STALE_DAYS: i64 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum IssueKind {
    DuplicatePath,
    NestedProject,
    MissingPath,
    BrokenWorkingDir,
    OrphanIndex,
    StaleIndex,
    StaleGraph,
}

/// A machine-applicable fix. `Reindex`/`RebuildGraph` are suggestions —
/// surfaces print the command instead of running a multi-minute job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum DoctorFix {
    RemoveProject { name: String },
    ClearWorkingDir { project: String, service: String },
    DeleteIndexDir { path: String },
    Reindex { project: String },
    RebuildGraph { project: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorIssue {
    pub kind: IssueKind,
    /// Project the issue belongs to (None for orphan artifacts).
    pub project: Option<String>,
    pub detail: String,
    pub fix: Option<DoctorFix>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub checked_projects: usize,
    pub issues: Vec<DoctorIssue>,
}

/// True when an orphan index dir looks like a test-fixture leftover
/// (`contracts-test-93042`, `deadcode-fixture-1181`, `test-cleanup`...).
/// `void doctor --fix` offers to delete these in ONE batch instead of one
/// y/N prompt per directory.
pub fn is_fixture_orphan(dir_name: &str) -> bool {
    fn re() -> &'static regex::Regex {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| regex::Regex::new(r"(?:^test-|-(?:test|fixture|macro)-\d+$)").unwrap())
    }
    re().is_match(dir_name)
}

/// Central semantic-index root (`<data_local_dir>/void-stack/indexes`).
pub fn indexes_root() -> PathBuf {
    let base = crate::global_config::data_base_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("void-stack").join("indexes")
}

/// Project name → canonical path (None when the path no longer exists).
type CanonPaths = Vec<(String, Option<PathBuf>)>;

/// Run every check against the registry. `indexes_root` is injectable so
/// tests can point it at a tempdir.
pub fn run_doctor(config: &GlobalConfig, indexes_root: &Path) -> DoctorReport {
    let mut issues = Vec::new();

    // Canonical path per project (missing paths canonicalize to None).
    let canon: CanonPaths = config
        .projects
        .iter()
        .map(|p| {
            let clean = strip_win_prefix(&p.path);
            (p.name.clone(), Path::new(&clean).canonicalize().ok())
        })
        .collect();

    check_missing_paths(config, &canon, &mut issues);
    check_duplicate_paths(&canon, &mut issues);
    check_nested_projects(&canon, &mut issues);
    check_broken_working_dirs(config, &mut issues);
    check_orphan_indexes(config, indexes_root, &mut issues);
    #[cfg(feature = "vector")]
    check_stale_indexes(config, &mut issues);
    #[cfg(feature = "structural")]
    check_stale_graphs(config, &mut issues);

    DoctorReport {
        checked_projects: config.projects.len(),
        issues,
    }
}

/// 1. Missing paths.
fn check_missing_paths(config: &GlobalConfig, canon: &CanonPaths, issues: &mut Vec<DoctorIssue>) {
    for (name, path) in canon {
        if path.is_none() {
            let raw = config
                .projects
                .iter()
                .find(|p| &p.name == name)
                .map(|p| p.path.clone())
                .unwrap_or_default();
            issues.push(DoctorIssue {
                kind: IssueKind::MissingPath,
                project: Some(name.clone()),
                detail: format!("path no longer exists: {}", raw),
                fix: Some(DoctorFix::RemoveProject { name: name.clone() }),
            });
        }
    }
}

/// 2. Duplicate canonical paths (first registration wins).
fn check_duplicate_paths(canon: &CanonPaths, issues: &mut Vec<DoctorIssue>) {
    let mut seen: HashMap<&Path, &str> = HashMap::new();
    for (name, path) in canon {
        let Some(path) = path else { continue };
        match seen.get(path.as_path()) {
            Some(first) => issues.push(DoctorIssue {
                kind: IssueKind::DuplicatePath,
                project: Some(name.clone()),
                detail: format!(
                    "same directory as '{}' ({}) — registered twice",
                    first,
                    path.display()
                ),
                fix: Some(DoctorFix::RemoveProject { name: name.clone() }),
            }),
            None => {
                seen.insert(path.as_path(), name);
            }
        }
    }
}

/// 3. Nested projects (report only — which one to keep is a human call).
fn check_nested_projects(canon: &CanonPaths, issues: &mut Vec<DoctorIssue>) {
    for (name, path) in canon {
        let Some(path) = path else { continue };
        for (other, other_path) in canon {
            let Some(other_path) = other_path else {
                continue;
            };
            if name != other && path != other_path && path.starts_with(other_path) {
                issues.push(DoctorIssue {
                    kind: IssueKind::NestedProject,
                    project: Some(name.clone()),
                    detail: format!(
                        "registered inside '{}' ({} ⊂ {})",
                        other,
                        path.display(),
                        other_path.display()
                    ),
                    fix: None,
                });
            }
        }
    }
}

/// 4. Broken service working dirs.
fn check_broken_working_dirs(config: &GlobalConfig, issues: &mut Vec<DoctorIssue>) {
    for project in &config.projects {
        for service in &project.services {
            if let Some(wd) = &service.working_dir {
                let clean = strip_win_prefix(wd);
                if !Path::new(&clean).exists() {
                    issues.push(DoctorIssue {
                        kind: IssueKind::BrokenWorkingDir,
                        project: Some(project.name.clone()),
                        detail: format!(
                            "service '{}' working_dir no longer exists: {}",
                            service.name, wd
                        ),
                        fix: Some(DoctorFix::ClearWorkingDir {
                            project: project.name.clone(),
                            service: service.name.clone(),
                        }),
                    });
                }
            }
        }
    }
}

/// 5. Orphan semantic indexes (dir name no longer matches any project).
fn check_orphan_indexes(config: &GlobalConfig, indexes_root: &Path, issues: &mut Vec<DoctorIssue>) {
    if let Ok(entries) = std::fs::read_dir(indexes_root) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            let registered = config
                .projects
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(&dir_name));
            if !registered {
                issues.push(DoctorIssue {
                    kind: IssueKind::OrphanIndex,
                    project: None,
                    detail: format!(
                        "semantic index for unregistered project '{}' ({})",
                        dir_name,
                        entry.path().display()
                    ),
                    fix: Some(DoctorFix::DeleteIndexDir {
                        path: entry.path().to_string_lossy().to_string(),
                    }),
                });
            }
        }
    }
}

/// 6. Stale semantic indexes (> STALE_DAYS).
#[cfg(feature = "vector")]
fn check_stale_indexes(config: &GlobalConfig, issues: &mut Vec<DoctorIssue>) {
    for project in &config.projects {
        if let Ok(Some(stats)) = crate::vector_index::get_index_stats(project) {
            let age = chrono::Utc::now().signed_duration_since(stats.created_at);
            if age.num_days() > STALE_DAYS {
                issues.push(DoctorIssue {
                    kind: IssueKind::StaleIndex,
                    project: Some(project.name.clone()),
                    detail: format!("semantic index is {} days old", age.num_days()),
                    fix: Some(DoctorFix::Reindex {
                        project: project.name.clone(),
                    }),
                });
            }
        }
    }
}

/// 7. Stale structural graphs (> STALE_DAYS by db mtime).
#[cfg(feature = "structural")]
fn check_stale_graphs(config: &GlobalConfig, issues: &mut Vec<DoctorIssue>) {
    for project in &config.projects {
        let db = crate::structural::structural_db_path(project);
        if !db.exists() {
            continue;
        }
        if let Ok(modified) = std::fs::metadata(&db).and_then(|m| m.modified()) {
            let ts = chrono::DateTime::<chrono::Utc>::from(modified);
            let age = chrono::Utc::now().signed_duration_since(ts);
            if age.num_days() > STALE_DAYS {
                issues.push(DoctorIssue {
                    kind: IssueKind::StaleGraph,
                    project: Some(project.name.clone()),
                    detail: format!("structural graph is {} days old", age.num_days()),
                    fix: Some(DoctorFix::RebuildGraph {
                        project: project.name.clone(),
                    }),
                });
            }
        }
    }
}

/// Apply one fix. Mutates `config` in memory (the caller saves it once at
/// the end) or the filesystem for index deletion. Returns a description of
/// what was done. `Reindex`/`RebuildGraph` return the command to run.
pub fn apply_fix(config: &mut GlobalConfig, fix: &DoctorFix) -> Result<String, String> {
    match fix {
        DoctorFix::RemoveProject { name } => {
            let before = config.projects.len();
            config
                .projects
                .retain(|p| !p.name.eq_ignore_ascii_case(name));
            if config.projects.len() == before {
                return Err(format!("project '{}' not found", name));
            }
            Ok(format!("removed project '{}' from the registry", name))
        }
        DoctorFix::ClearWorkingDir { project, service } => {
            let svc = config
                .projects
                .iter_mut()
                .find(|p| p.name.eq_ignore_ascii_case(project))
                .and_then(|p| {
                    p.services
                        .iter_mut()
                        .find(|s| s.name.eq_ignore_ascii_case(service))
                })
                .ok_or_else(|| format!("service '{}/{}' not found", project, service))?;
            svc.working_dir = None;
            Ok(format!(
                "cleared working_dir of '{}/{}' (falls back to the project root)",
                project, service
            ))
        }
        DoctorFix::DeleteIndexDir { path } => {
            // Only ever delete inside a .../void-stack/indexes/ tree.
            let p = Path::new(path);
            let inside_indexes = p
                .parent()
                .map(|parent| parent.ends_with(Path::new("void-stack").join("indexes")))
                .unwrap_or(false);
            if !inside_indexes {
                return Err(format!(
                    "refusing to delete outside an indexes dir: {}",
                    path
                ));
            }
            std::fs::remove_dir_all(p).map_err(|e| format!("cannot delete {}: {}", path, e))?;
            Ok(format!("deleted orphan index {}", path))
        }
        DoctorFix::Reindex { project } => Ok(format!("run: void index {} --force", project)),
        DoctorFix::RebuildGraph { project } => {
            Ok(format!("run: void graph-build {} --force", project))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, Service, Target};

    fn project(name: &str, path: &Path) -> Project {
        Project {
            name: name.to_string(),
            path: path.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    fn issues_of(report: &DoctorReport, kind: IssueKind) -> Vec<&DoctorIssue> {
        report.issues.iter().filter(|i| i.kind == kind).collect()
    }

    #[test]
    fn test_is_fixture_orphan_patterns() {
        for name in [
            "contracts-test-93042",
            "deadcode-fixture-1181",
            "hybrid-fixture-7",
            "deadcode-macro-42",
            "test-cleanup",
            "test-order-orphan",
        ] {
            assert!(is_fixture_orphan(name), "{name} must match");
        }
        for name in [
            "void-stack",
            "iunci-flutter",
            "my-test-project",
            "attest-app",
        ] {
            assert!(!is_fixture_orphan(name), "{name} must NOT match");
        }
    }

    #[test]
    fn test_doctor_clean_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        let config = GlobalConfig {
            projects: vec![project("a", &a)],
            ..Default::default()
        };
        let report = run_doctor(&config, &tmp.path().join("indexes"));
        assert_eq!(report.checked_projects, 1);
        assert!(report.issues.is_empty(), "{:?}", report.issues);
    }

    #[test]
    fn test_doctor_duplicate_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("app");
        std::fs::create_dir_all(&a).unwrap();
        let config = GlobalConfig {
            projects: vec![project("iws-core-api", &a), project("iws-core-api-2", &a)],
            ..Default::default()
        };
        let report = run_doctor(&config, &tmp.path().join("indexes"));
        let dups = issues_of(&report, IssueKind::DuplicatePath);
        assert_eq!(dups.len(), 1);
        // The LATER registration is the removable one.
        assert_eq!(dups[0].project.as_deref(), Some("iws-core-api-2"));
        assert!(matches!(
            dups[0].fix,
            Some(DoctorFix::RemoveProject { ref name }) if name == "iws-core-api-2"
        ));
    }

    #[test]
    fn test_doctor_nested_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().join("glowing-robot");
        let inner = outer.join("sentinel-search");
        std::fs::create_dir_all(&inner).unwrap();
        let config = GlobalConfig {
            projects: vec![
                project("glowing-robot", &outer),
                project("sentinel-search", &inner),
            ],
            ..Default::default()
        };
        let report = run_doctor(&config, &tmp.path().join("indexes"));
        let nested = issues_of(&report, IssueKind::NestedProject);
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].project.as_deref(), Some("sentinel-search"));
        assert!(nested[0].fix.is_none(), "nested is report-only");
    }

    #[test]
    fn test_doctor_missing_path_and_fix() {
        let tmp = tempfile::tempdir().unwrap();
        let gone = tmp.path().join("gone");
        let mut config = GlobalConfig {
            projects: vec![project("ghost", &gone)],
            ..Default::default()
        };
        let report = run_doctor(&config, &tmp.path().join("indexes"));
        let missing = issues_of(&report, IssueKind::MissingPath);
        assert_eq!(missing.len(), 1);
        let fix = missing[0].fix.clone().unwrap();
        let msg = apply_fix(&mut config, &fix).unwrap();
        assert!(msg.contains("ghost"));
        assert!(config.projects.is_empty());
    }

    #[test]
    fn test_doctor_broken_working_dir_and_fix() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        let mut p = project("a", &a);
        p.services.push(Service {
            name: "api".into(),
            command: "cargo run".into(),
            target: Target::Windows,
            working_dir: Some(a.join("deleted-subdir").to_string_lossy().to_string()),
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
            docker: None,
        });
        let mut config = GlobalConfig {
            projects: vec![p],
            ..Default::default()
        };
        let report = run_doctor(&config, &tmp.path().join("indexes"));
        let broken = issues_of(&report, IssueKind::BrokenWorkingDir);
        assert_eq!(broken.len(), 1);
        let fix = broken[0].fix.clone().unwrap();
        apply_fix(&mut config, &fix).unwrap();
        assert_eq!(config.projects[0].services[0].working_dir, None);
    }

    #[test]
    fn test_doctor_orphan_index_and_fix_guard() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        // Emulate the central layout: <root>/void-stack/indexes/<name>.
        let indexes = tmp.path().join("void-stack").join("indexes");
        std::fs::create_dir_all(indexes.join("a")).unwrap();
        std::fs::create_dir_all(indexes.join("removed-project")).unwrap();
        let mut config = GlobalConfig {
            projects: vec![project("a", &a)],
            ..Default::default()
        };
        let report = run_doctor(&config, &indexes);
        let orphans = issues_of(&report, IssueKind::OrphanIndex);
        assert_eq!(orphans.len(), 1);
        assert!(orphans[0].detail.contains("removed-project"));

        let fix = orphans[0].fix.clone().unwrap();
        apply_fix(&mut config, &fix).unwrap();
        assert!(!indexes.join("removed-project").exists());
        assert!(indexes.join("a").exists());

        // The guard refuses paths outside an indexes tree.
        let bad = DoctorFix::DeleteIndexDir {
            path: a.to_string_lossy().to_string(),
        };
        assert!(apply_fix(&mut config, &bad).is_err());
        assert!(a.exists());
    }
}
