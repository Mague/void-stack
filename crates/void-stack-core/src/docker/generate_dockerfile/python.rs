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
