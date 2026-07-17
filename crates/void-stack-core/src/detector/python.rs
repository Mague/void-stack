use std::path::Path;

use async_trait::async_trait;

use super::{CheckStatus, DependencyDetector, DependencyStatus, DependencyType, run_cmd};

pub struct PythonDetector;

#[async_trait]
impl DependencyDetector for PythonDetector {
    fn dep_type(&self) -> DependencyType {
        DependencyType::Python
    }

    fn is_relevant(&self, project_path: &Path) -> bool {
        project_path.join("requirements.txt").exists()
            || project_path.join("pyproject.toml").exists()
            || project_path.join("setup.py").exists()
    }

    async fn check(&self, project_path: &Path) -> DependencyStatus {
        let mut status = DependencyStatus::ok(DependencyType::Python);

        // Check python binary (try python, python3, py)
        let mut python_version = None;
        for cmd in &["python", "python3", "py"] {
            if let Some(ver) = run_cmd(cmd, &["--version"]).await {
                python_version = Some(ver);
                break;
            }
        }
        match python_version {
            Some(ver) => {
                // "Python 3.11.5" → "3.11.5"
                let ver_clean = ver.strip_prefix("Python ").unwrap_or(&ver).to_string();
                status.version = Some(ver_clean.clone());
                status.details.push(format!("Python {}", ver_clean));
            }
            None => {
                // Also check if venv has python
                let venv_python = find_venv_python(project_path);
                if let Some(venv_py) = venv_python {
                    let venv_ver = run_cmd(&venv_py, &["--version"]).await;
                    if let Some(ver) = venv_ver {
                        let ver_clean = ver.strip_prefix("Python ").unwrap_or(&ver).to_string();
                        status.version = Some(ver_clean.clone());
                        status
                            .details
                            .push(format!("Python {} (venv only)", ver_clean));
                        // Python found in venv but not globally — still ok for VoidStack
                    } else {
                        return DependencyStatus {
                            dep_type: DependencyType::Python,
                            status: CheckStatus::Missing,
                            version: None,
                            details: vec!["Python not found in PATH or virtualenv".into()],
                            fix_hint: Some(crate::process_util::install_hint("python")),
                        };
                    }
                } else {
                    return DependencyStatus {
                        dep_type: DependencyType::Python,
                        status: CheckStatus::Missing,
                        version: None,
                        details: vec!["Python not found in PATH".into()],
                        fix_hint: Some(crate::process_util::install_hint("python")),
                    };
                }
            }
        }

        // Check for virtualenv
        let venv_dirs = [".venv", "venv", "env"];
        let mut venv_found = false;
        for venv in &venv_dirs {
            let venv_path = project_path.join(venv);
            let has_scripts = venv_path.join("Scripts").exists() || venv_path.join("bin").exists();
            if has_scripts {
                status.details.push(format!("Virtualenv: {}/", venv));
                venv_found = true;
                break;
            }
        }

        if !venv_found {
            // Check parent directories (monorepo)
            let mut current = project_path.parent();
            while let Some(parent) = current {
                for venv in &venv_dirs {
                    let venv_path = parent.join(venv);
                    if venv_path.join("Scripts").exists() || venv_path.join("bin").exists() {
                        let rel = venv_path
                            .strip_prefix(project_path)
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| venv_path.display().to_string());
                        status
                            .details
                            .push(format!("Virtualenv: {} (ancestor)", rel));
                        venv_found = true;
                        break;
                    }
                }
                if venv_found {
                    break;
                }
                current = parent.parent();
                // Limit to 4 levels up
                if current.map(|p| p.components().count()).unwrap_or(0)
                    < project_path.components().count().saturating_sub(4)
                {
                    break;
                }
            }
        }

        if !venv_found {
            status.status = CheckStatus::NeedsSetup;
            status.details.push("No virtualenv found".into());
            status.fix_hint = Some("python -m venv .venv".into());
            return status;
        }

        // Check if requirements are installed (quick pip check)
        if project_path.join("requirements.txt").exists() {
            // Find pip in the venv
            let pip = find_venv_pip(project_path);
            if let Some(pip_path) = pip {
                let check = run_cmd(&pip_path, &["check"]).await;
                match check {
                    Some(out) if out.contains("No broken requirements") => {
                        status.details.push("pip check: OK".into());
                    }
                    Some(out) => {
                        status.status = CheckStatus::NeedsSetup;
                        let first_line = out.lines().next().unwrap_or(&out);
                        status.details.push(format!("pip check: {}", first_line));
                        status.fix_hint = Some("pip install -r requirements.txt".into());
                    }
                    None => {
                        status.details.push("pip check: could not run".into());
                    }
                }
            }
        }

        status
    }
}

fn find_venv_python(project_path: &Path) -> Option<String> {
    let venv_dirs = [".venv", "venv", "env"];
    let mut search = vec![project_path.to_path_buf()];
    let mut current = project_path.parent();
    for _ in 0..4 {
        if let Some(parent) = current {
            search.push(parent.to_path_buf());
            current = parent.parent();
        }
    }
    for dir in &search {
        for venv in &venv_dirs {
            let py = dir.join(venv).join("Scripts").join("python.exe");
            if py.exists() {
                return Some(py.display().to_string());
            }
            let py = dir.join(venv).join("bin").join("python");
            if py.exists() {
                return Some(py.display().to_string());
            }
        }
    }
    None
}

fn find_venv_pip(project_path: &Path) -> Option<String> {
    let venv_dirs = [".venv", "venv", "env"];

    // Check in project dir and ancestors
    let mut search = vec![project_path.to_path_buf()];
    let mut current = project_path.parent();
    for _ in 0..4 {
        if let Some(parent) = current {
            search.push(parent.to_path_buf());
            current = parent.parent();
        }
    }

    for dir in &search {
        for venv in &venv_dirs {
            let pip = dir.join(venv).join("Scripts").join("pip.exe");
            if pip.exists() {
                return Some(pip.display().to_string());
            }
            let pip = dir.join(venv).join("bin").join("pip");
            if pip.exists() {
                return Some(pip.display().to_string());
            }
        }
    }
    None
}

// NOTE: `PythonDetector::check` is intentionally not tested here — it shells
// out to real `python`/`pip` binaries, so its result depends on the host.
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Create an empty file, creating parent directories as needed.
    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn test_dep_type_is_python() {
        assert_eq!(
            PythonDetector.dep_type(),
            DependencyType::Python,
            "detector should report the Python dependency type"
        );
    }

    // ── is_relevant ──

    #[test]
    fn test_is_relevant_with_requirements_txt() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();

        assert!(
            PythonDetector.is_relevant(dir.path()),
            "requirements.txt should mark the project as Python"
        );
    }

    #[test]
    fn test_is_relevant_with_pyproject_toml() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"x\"\n",
        )
        .unwrap();

        assert!(
            PythonDetector.is_relevant(dir.path()),
            "pyproject.toml should mark the project as Python"
        );
    }

    #[test]
    fn test_is_relevant_with_setup_py() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("setup.py"),
            "from setuptools import setup\n",
        )
        .unwrap();

        assert!(
            PythonDetector.is_relevant(dir.path()),
            "setup.py should mark the project as Python"
        );
    }

    #[test]
    fn test_is_relevant_false_for_empty_dir() {
        let dir = tempdir().unwrap();
        assert!(
            !PythonDetector.is_relevant(dir.path()),
            "a directory without Python manifests should not be relevant"
        );
    }

    // ── find_venv_python ──

    #[test]
    fn test_find_venv_python_in_project_bin() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join(".venv").join("bin").join("python"));

        let found = find_venv_python(dir.path());
        assert!(found.is_some(), "python inside .venv/bin should be found");
        assert!(
            found.unwrap().contains(".venv"),
            "returned path should point into the .venv directory"
        );
    }

    #[test]
    fn test_find_venv_python_in_project_scripts() {
        // Windows-style venv layout (Scripts/python.exe)
        let dir = tempdir().unwrap();
        touch(&dir.path().join("venv").join("Scripts").join("python.exe"));

        let found = find_venv_python(dir.path());
        assert!(
            found.is_some(),
            "python.exe inside venv/Scripts should be found"
        );
        assert!(
            found.unwrap().contains("python.exe"),
            "returned path should be the Windows interpreter"
        );
    }

    #[test]
    fn test_find_venv_python_in_ancestor_dir() {
        // Monorepo layout: venv lives at the repo root, project is nested
        let root = tempdir().unwrap();
        touch(&root.path().join(".venv").join("bin").join("python"));
        let project = root.path().join("services").join("api");
        fs::create_dir_all(&project).unwrap();

        let found = find_venv_python(&project);
        assert!(
            found.is_some(),
            "venv in an ancestor directory should be found"
        );
    }

    #[test]
    fn test_find_venv_python_none_when_absent() {
        // Nest the project deep enough that the 4-level ancestor walk
        // stays inside the temp directory.
        let root = tempdir().unwrap();
        let project = root
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e");
        fs::create_dir_all(&project).unwrap();

        assert!(
            find_venv_python(&project).is_none(),
            "no venv anywhere should yield None"
        );
    }

    // ── find_venv_pip ──

    #[test]
    fn test_find_venv_pip_in_project() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join(".venv").join("bin").join("pip"));

        let found = find_venv_pip(dir.path());
        assert!(found.is_some(), "pip inside .venv/bin should be found");
    }

    #[test]
    fn test_find_venv_pip_alternate_venv_dir_names() {
        // "env" is the last alternative directory name that is scanned
        let dir = tempdir().unwrap();
        touch(&dir.path().join("env").join("Scripts").join("pip.exe"));

        let found = find_venv_pip(dir.path());
        assert!(found.is_some(), "pip inside env/Scripts should be found");
        assert!(
            found.unwrap().contains("env"),
            "returned path should point into the env directory"
        );
    }

    #[test]
    fn test_find_venv_pip_none_when_absent() {
        let root = tempdir().unwrap();
        let project = root
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e");
        fs::create_dir_all(&project).unwrap();

        assert!(
            find_venv_pip(&project).is_none(),
            "no venv anywhere should yield None"
        );
    }
}
