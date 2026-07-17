//! Python Dockerfile generation (FastAPI, Flask, Django).

use std::path::Path;

pub(super) fn python_dockerfile(path: &Path) -> String {
    let python_version = detect_python_version(path);
    let framework = detect_python_framework(path);

    let (entrypoint, port) = match framework.as_str() {
        "fastapi" => (
            "uvicorn main:app --host 0.0.0.0 --port 8000".to_string(),
            8000,
        ),
        "flask" => ("gunicorn -w 4 -b 0.0.0.0:5000 app:app".to_string(), 5000),
        "django" => (
            "gunicorn -w 4 -b 0.0.0.0:8000 config.wsgi:application".to_string(),
            8000,
        ),
        _ => ("python main.py".to_string(), 8000),
    };

    let deps_file = if path.join("requirements.txt").exists() {
        "requirements.txt"
    } else if path.join("pyproject.toml").exists() {
        "pyproject.toml"
    } else {
        "requirements.txt"
    };

    let install_cmd = if deps_file == "pyproject.toml" {
        "pip install --no-cache-dir ."
    } else {
        "pip install --no-cache-dir -r requirements.txt"
    };

    format!(
        r#"# ── Build stage ──
FROM python:{python_version}-slim AS builder

WORKDIR /app

RUN python -m venv /opt/venv
ENV PATH="/opt/venv/bin:$PATH"

COPY {deps_file} .
RUN {install_cmd}

# ── Runtime stage ──
FROM python:{python_version}-slim

RUN groupadd -r app && useradd -r -g app app

WORKDIR /app

COPY --from=builder /opt/venv /opt/venv
ENV PATH="/opt/venv/bin:$PATH"

COPY --chown=app:app . .

USER app

EXPOSE {port}

CMD [{cmd_array}]
"#,
        python_version = python_version,
        deps_file = deps_file,
        install_cmd = install_cmd,
        port = port,
        cmd_array = entrypoint
            .split_whitespace()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn detect_python_version(path: &Path) -> String {
    if let Ok(v) = std::fs::read_to_string(path.join(".python-version")) {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }
    if let Ok(content) = std::fs::read_to_string(path.join("pyproject.toml")) {
        for line in content.lines() {
            if line.contains("requires-python")
                && let Some(ver) = line.split('"').nth(1)
            {
                let clean = ver.trim_start_matches(['>', '=', '<', '~', '^']);
                if !clean.is_empty() {
                    return clean.to_string();
                }
            }
        }
    }
    "3.12".to_string()
}

fn detect_python_framework(path: &Path) -> String {
    let files = ["requirements.txt", "pyproject.toml", "Pipfile"];
    for file in &files {
        if let Ok(content) = std::fs::read_to_string(path.join(file)) {
            let lower = content.to_lowercase();
            if lower.contains("fastapi") {
                return "fastapi".to_string();
            }
            if lower.contains("flask") {
                return "flask".to_string();
            }
            if lower.contains("django") {
                return "django".to_string();
            }
        }
    }
    "generic".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── detect_python_version ──

    #[test]
    fn test_detect_python_version_from_version_file_is_trimmed() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".python-version"), "3.10.4\n").unwrap();

        assert_eq!(
            detect_python_version(dir.path()),
            "3.10.4",
            "trailing newline in .python-version should be trimmed"
        );
    }

    #[test]
    fn test_detect_python_version_from_pyproject_requires_python() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"app\"\nrequires-python = \">=3.11\"\n",
        )
        .unwrap();

        assert_eq!(
            detect_python_version(dir.path()),
            "3.11",
            "requires-python constraint operators should be stripped"
        );
    }

    #[test]
    fn test_detect_python_version_defaults_when_nothing_found() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_python_version(dir.path()),
            "3.12",
            "empty project should fall back to the default version"
        );
    }

    // ── detect_python_framework ──

    #[test]
    fn test_detect_python_framework_fastapi_from_requirements() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "FastAPI==0.110.0\n").unwrap();

        assert_eq!(
            detect_python_framework(dir.path()),
            "fastapi",
            "detection should be case-insensitive"
        );
    }

    #[test]
    fn test_detect_python_framework_flask_from_pipfile() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Pipfile"), "[packages]\nflask = \"*\"\n").unwrap();

        assert_eq!(
            detect_python_framework(dir.path()),
            "flask",
            "Pipfile should also be scanned for frameworks"
        );
    }

    #[test]
    fn test_detect_python_framework_django_from_pyproject() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\ndependencies = [\"django\"]\n",
        )
        .unwrap();

        assert_eq!(detect_python_framework(dir.path()), "django");
    }

    #[test]
    fn test_detect_python_framework_generic_without_dep_files() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_python_framework(dir.path()),
            "generic",
            "no dependency manifest should mean generic"
        );
    }

    // ── python_dockerfile end-to-end ──

    #[test]
    fn test_python_dockerfile_fastapi_uses_uvicorn() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "fastapi\nuvicorn\n").unwrap();

        let dockerfile = python_dockerfile(dir.path());
        assert!(
            dockerfile.contains("\"uvicorn\", \"main:app\""),
            "FastAPI should launch via uvicorn: {dockerfile}"
        );
        assert!(
            dockerfile.contains("EXPOSE 8000"),
            "FastAPI listens on 8000"
        );
        assert!(
            dockerfile.contains("pip install --no-cache-dir -r requirements.txt"),
            "requirements.txt should drive the install command"
        );
        assert!(
            dockerfile.contains("USER app"),
            "should run as non-root user"
        );
    }

    #[test]
    fn test_python_dockerfile_flask_uses_gunicorn_on_5000() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\ngunicorn\n").unwrap();

        let dockerfile = python_dockerfile(dir.path());
        assert!(
            dockerfile.contains("\"gunicorn\""),
            "Flask should launch via gunicorn"
        );
        assert!(dockerfile.contains("EXPOSE 5000"), "Flask listens on 5000");
    }

    #[test]
    fn test_python_dockerfile_django_uses_wsgi() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "django\ngunicorn\n").unwrap();

        let dockerfile = python_dockerfile(dir.path());
        assert!(
            dockerfile.contains("config.wsgi:application"),
            "Django should launch the WSGI application"
        );
        assert!(dockerfile.contains("EXPOSE 8000"), "Django listens on 8000");
    }

    #[test]
    fn test_python_dockerfile_pyproject_installs_project() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"svc\"\nrequires-python = \">=3.11\"\ndependencies = [\"fastapi\"]\n",
        )
        .unwrap();

        let dockerfile = python_dockerfile(dir.path());
        assert!(
            dockerfile.contains("COPY pyproject.toml ."),
            "pyproject.toml should be the copied manifest"
        );
        assert!(
            dockerfile.contains("pip install --no-cache-dir ."),
            "pyproject projects should install the package itself"
        );
        assert!(
            dockerfile.contains("FROM python:3.11-slim"),
            "requires-python should pick the base image version"
        );
    }

    #[test]
    fn test_python_dockerfile_generic_defaults() {
        let dir = tempdir().unwrap();

        let dockerfile = python_dockerfile(dir.path());
        assert!(
            dockerfile.contains("FROM python:3.12-slim"),
            "default version should be 3.12"
        );
        assert!(
            dockerfile.contains("\"python\", \"main.py\""),
            "generic project should run main.py"
        );
        assert!(
            dockerfile.contains("EXPOSE 8000"),
            "generic default port is 8000"
        );
    }
}
