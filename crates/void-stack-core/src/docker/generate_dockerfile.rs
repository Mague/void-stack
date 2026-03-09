//! Generate Dockerfiles based on detected project framework.

use std::path::Path;

use crate::model::ProjectType;

/// Generate a Dockerfile for the given project type. Returns None if unsupported.
pub fn generate(project_path: &Path, project_type: ProjectType) -> Option<String> {
    match project_type {
        ProjectType::Python => Some(python_dockerfile(project_path)),
        ProjectType::Node => Some(node_dockerfile(project_path)),
        ProjectType::Rust => Some(rust_dockerfile(project_path)),
        ProjectType::Go => Some(go_dockerfile(project_path)),
        ProjectType::Flutter => Some(flutter_web_dockerfile(project_path)),
        _ => None,
    }
}

// ── Python ──

fn python_dockerfile(path: &Path) -> String {
    let python_version = detect_python_version(path);
    let framework = detect_python_framework(path);

    let (entrypoint, port) = match framework.as_str() {
        "fastapi" => ("uvicorn main:app --host 0.0.0.0 --port 8000".to_string(), 8000),
        "flask" => ("gunicorn -w 4 -b 0.0.0.0:5000 app:app".to_string(), 5000),
        "django" => ("gunicorn -w 4 -b 0.0.0.0:8000 config.wsgi:application".to_string(), 8000),
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

# Install dependencies in a virtual environment
RUN python -m venv /opt/venv
ENV PATH="/opt/venv/bin:$PATH"

COPY {deps_file} .
RUN {install_cmd}

# ── Runtime stage ──
FROM python:{python_version}-slim

WORKDIR /app

# Copy virtual environment from builder
COPY --from=builder /opt/venv /opt/venv
ENV PATH="/opt/venv/bin:$PATH"

# Copy application code
COPY . .

EXPOSE {port}

CMD [{cmd_array}]
"#,
        python_version = python_version,
        deps_file = deps_file,
        install_cmd = install_cmd,
        port = port,
        cmd_array = entrypoint.split_whitespace()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn detect_python_version(path: &Path) -> String {
    // Try .python-version
    if let Ok(v) = std::fs::read_to_string(path.join(".python-version")) {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }
    // Try pyproject.toml for requires-python
    if let Ok(content) = std::fs::read_to_string(path.join("pyproject.toml")) {
        for line in content.lines() {
            if line.contains("requires-python") {
                // requires-python = ">=3.11" → 3.11
                if let Some(ver) = line.split('"').nth(1) {
                    let clean = ver.trim_start_matches(['>', '=', '<', '~', '^']);
                    if !clean.is_empty() {
                        return clean.to_string();
                    }
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
            if lower.contains("fastapi") { return "fastapi".to_string(); }
            if lower.contains("flask") { return "flask".to_string(); }
            if lower.contains("django") { return "django".to_string(); }
        }
    }
    "generic".to_string()
}

// ── Node.js ──

fn node_dockerfile(path: &Path) -> String {
    let node_version = detect_node_version(path);
    let framework = detect_node_framework(path);
    let pkg_manager = detect_node_pkg_manager(path);

    let (install_cmd, lock_file) = match pkg_manager.as_str() {
        "pnpm" => ("pnpm install --frozen-lockfile", "pnpm-lock.yaml"),
        "yarn" => ("yarn install --frozen-lockfile", "yarn.lock"),
        _ => ("npm ci", "package-lock.json"),
    };

    let (build_cmd, start_cmd, port) = match framework.as_str() {
        "next" => ("npm run build", "npm start", 3000),
        "vite" | "react" => ("npm run build", "npx serve -s dist -l 3000", 3000),
        _ => ("", "node server.js", 3000),
    };

    let build_stage = if !build_cmd.is_empty() {
        format!(
            r#"
# ── Build stage ──
FROM node:{node_version}-alpine AS builder

WORKDIR /app

COPY package.json {lock_file} ./
RUN {install_cmd}

COPY . .
RUN {build_cmd}

"#,
            node_version = node_version,
            lock_file = lock_file,
            install_cmd = install_cmd,
            build_cmd = build_cmd,
        )
    } else {
        String::new()
    };

    let runtime = if framework == "vite" || framework == "react" {
        // Static build → nginx
        format!(
            r#"# ── Runtime stage ──
FROM nginx:alpine

COPY --from=builder /app/dist /usr/share/nginx/html

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
"#
        )
    } else if !build_cmd.is_empty() {
        format!(
            r#"# ── Runtime stage ──
FROM node:{node_version}-alpine

WORKDIR /app

COPY --from=builder /app/package.json ./
COPY --from=builder /app/node_modules ./node_modules
COPY --from=builder /app/.next ./.next
COPY --from=builder /app/public ./public

EXPOSE {port}

CMD [{cmd}]
"#,
            node_version = node_version,
            port = port,
            cmd = start_cmd.split_whitespace()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", "),
        )
    } else {
        format!(
            r#"# ── Runtime ──
FROM node:{node_version}-alpine

WORKDIR /app

COPY package.json {lock_file} ./
RUN {install_cmd} --production

COPY . .

EXPOSE {port}

CMD [{cmd}]
"#,
            node_version = node_version,
            lock_file = lock_file,
            install_cmd = install_cmd,
            port = port,
            cmd = start_cmd.split_whitespace()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", "),
        )
    };

    format!("{}{}", build_stage, runtime)
}

fn detect_node_version(path: &Path) -> String {
    if let Ok(v) = std::fs::read_to_string(path.join(".nvmrc")) {
        let v = v.trim().trim_start_matches('v');
        if !v.is_empty() {
            return v.to_string();
        }
    }
    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(engines) = pkg.get("engines").and_then(|e| e.get("node")).and_then(|n| n.as_str()) {
                let clean = engines.trim_start_matches(['>', '=', '<', '~', '^']);
                if let Some(major) = clean.split('.').next() {
                    if major.parse::<u32>().is_ok() {
                        return major.to_string();
                    }
                }
            }
        }
    }
    "20".to_string()
}

fn detect_node_framework(path: &Path) -> String {
    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        let lower = content.to_lowercase();
        if lower.contains("\"next\"") { return "next".to_string(); }
        if lower.contains("\"vite\"") { return "vite".to_string(); }
        if lower.contains("\"react-scripts\"") { return "react".to_string(); }
        if lower.contains("\"express\"") { return "express".to_string(); }
    }
    "generic".to_string()
}

fn detect_node_pkg_manager(path: &Path) -> String {
    if path.join("pnpm-lock.yaml").exists() { return "pnpm".to_string(); }
    if path.join("yarn.lock").exists() { return "yarn".to_string(); }
    "npm".to_string()
}

// ── Rust ──

fn rust_dockerfile(path: &Path) -> String {
    let bin_name = detect_rust_bin_name(path);

    format!(
        r#"# ── Chef stage (dependency caching) ──
FROM rust:1.83 AS chef
RUN cargo install cargo-chef
WORKDIR /app

# ── Plan dependencies ──
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Build dependencies (cached) ──
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release --bin {bin_name}

# ── Runtime stage ──
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/{bin_name} .

EXPOSE 8080

CMD ["./{bin_name}"]
"#,
        bin_name = bin_name,
    )
}

fn detect_rust_bin_name(path: &Path) -> String {
    if let Ok(content) = std::fs::read_to_string(path.join("Cargo.toml")) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("name") {
                if let Some(name) = trimmed.split('"').nth(1) {
                    return name.to_string();
                }
            }
        }
    }
    "app".to_string()
}

// ── Go ──

fn go_dockerfile(path: &Path) -> String {
    let _module_name = detect_go_module(path);

    format!(
        r#"# ── Build stage ──
FROM golang:1.22-alpine AS builder

WORKDIR /app

# Cache dependencies
COPY go.mod go.sum ./
RUN go mod download

# Build
COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o /app/server .

# ── Runtime stage ──
FROM gcr.io/distroless/static-debian12

WORKDIR /app
COPY --from=builder /app/server .

EXPOSE 8080

CMD ["/app/server"]
"#,
    )
}

fn detect_go_module(path: &Path) -> String {
    if let Ok(content) = std::fs::read_to_string(path.join("go.mod")) {
        if let Some(line) = content.lines().next() {
            if let Some(name) = line.strip_prefix("module ") {
                return name.trim().to_string();
            }
        }
    }
    "app".to_string()
}

// ── Flutter Web ──

fn flutter_web_dockerfile(_path: &Path) -> String {
    r#"# ── Build stage ──
FROM ghcr.io/cirruslabs/flutter:stable AS builder

WORKDIR /app
COPY . .

RUN flutter pub get
RUN flutter build web --release

# ── Runtime stage ──
FROM nginx:alpine

COPY --from=builder /app/build/web /usr/share/nginx/html

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
"#.to_string()
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_generate_python_fastapi() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("requirements.txt")).unwrap();
        write!(f, "fastapi==0.110.0\nuvicorn\npydantic\n").unwrap();

        let result = generate(dir.path(), ProjectType::Python).unwrap();
        assert!(result.contains("FROM python:"));
        assert!(result.contains("\"uvicorn\""));
        assert!(result.contains("EXPOSE 8000"));
        assert!(result.contains("AS builder"));
        assert!(result.contains("/opt/venv"));
    }

    #[test]
    fn test_generate_node_vite() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"my-app","dependencies":{"vite":"^5.0.0","react":"^18"}}"#,
        ).unwrap();

        let result = generate(dir.path(), ProjectType::Node).unwrap();
        assert!(result.contains("FROM node:"));
        assert!(result.contains("npm run build"));
        assert!(result.contains("nginx"));
    }

    #[test]
    fn test_generate_rust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-server\"\nversion = \"0.1.0\"\n",
        ).unwrap();

        let result = generate(dir.path(), ProjectType::Rust).unwrap();
        assert!(result.contains("cargo-chef"));
        assert!(result.contains("my-server"));
        assert!(result.contains("debian:bookworm-slim"));
    }

    #[test]
    fn test_generate_go() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module github.com/user/myapp\n\ngo 1.22\n").unwrap();

        let result = generate(dir.path(), ProjectType::Go).unwrap();
        assert!(result.contains("golang:1.22"));
        assert!(result.contains("distroless"));
        assert!(result.contains("CGO_ENABLED=0"));
    }

    #[test]
    fn test_generate_flutter() {
        let dir = tempfile::tempdir().unwrap();

        let result = generate(dir.path(), ProjectType::Flutter).unwrap();
        assert!(result.contains("flutter build web"));
        assert!(result.contains("nginx"));
    }

    #[test]
    fn test_unsupported_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(generate(dir.path(), ProjectType::Docker).is_none());
        assert!(generate(dir.path(), ProjectType::Unknown).is_none());
    }
}
