//! Generate production-grade Dockerfiles based on detected project framework.
//!
//! Templates follow official best practices:
//! - Docker docs: multi-stage builds, layer caching, non-root users
//! - Astro docs: https://docs.astro.build/en/recipes/docker/
//! - Next.js: https://github.com/vercel/next.js/blob/canary/examples/with-docker/Dockerfile
//! - Docker official images: https://github.com/docker-library/official-images

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

/// Generate a .dockerignore file content for the given project type.
pub fn generate_dockerignore(project_type: ProjectType) -> String {
    let mut lines = vec![
        "# Version control",
        ".git",
        ".gitignore",
        "",
        "# IDE",
        ".vscode",
        ".idea",
        "*.swp",
        "",
        "# Docker",
        "Dockerfile",
        "docker-compose*.yml",
        ".dockerignore",
        "",
        "# Docs",
        "README.md",
        "LICENSE",
        "CHANGELOG.md",
    ];

    match project_type {
        ProjectType::Node => {
            lines.extend_from_slice(&[
                "",
                "# Node",
                "node_modules",
                ".next",
                "dist",
                ".env*",
                "*.log",
            ]);
        }
        ProjectType::Python => {
            lines.extend_from_slice(&[
                "",
                "# Python",
                "__pycache__",
                "*.pyc",
                ".venv",
                "venv",
                ".env*",
            ]);
        }
        ProjectType::Rust => {
            lines.extend_from_slice(&[
                "",
                "# Rust",
                "target",
            ]);
        }
        ProjectType::Go => {
            lines.extend_from_slice(&[
                "",
                "# Go",
                "vendor",
            ]);
        }
        _ => {}
    }

    lines.join("\n")
}

// ── Python ──
// Best practices: venv in builder, slim base, non-root user

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
        cmd_array = entrypoint.split_whitespace()
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
            if line.contains("requires-python") {
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
// Best practices: multi-stage, auto-detect pkg manager, tsc bypass,
// framework-specific runtime (nginx for static, node for SSR/Next)

fn node_dockerfile(path: &Path) -> String {
    let node_version = detect_node_version(path);
    let framework = detect_node_framework(path);
    let pkg_manager = detect_node_pkg_manager(path);

    match framework.as_str() {
        "astro" => astro_dockerfile(path, &node_version, &pkg_manager),
        "next" => next_dockerfile(path, &node_version, &pkg_manager),
        "vite" | "react" => vite_dockerfile(path, &node_version, &pkg_manager),
        _ => generic_node_dockerfile(path, &node_version, &pkg_manager),
    }
}

/// Astro static site → multi-stage build + nginx
/// Reference: https://docs.astro.build/en/recipes/docker/
fn astro_dockerfile(path: &Path, node_version: &str, pkg_manager: &str) -> String {
    let (install_cmd, lock_copy) = pkg_install_cmds(pkg_manager);
    let build_cmd = detect_safe_build_cmd(path, "astro");
    let is_ssr = detect_astro_ssr(path);

    if is_ssr {
        // SSR mode → Node.js runtime
        format!(
            r#"# ── Dependencies ──
FROM node:{node_version}-alpine AS deps
WORKDIR /app
{lock_copy}
RUN {install_cmd}

# ── Build ──
FROM node:{node_version}-alpine AS builder
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
RUN {build_cmd}

# ── Runtime ──
FROM node:{node_version}-alpine

WORKDIR /app

COPY --from=deps /app/node_modules ./node_modules
COPY --from=builder /app/dist ./dist

ENV HOST=0.0.0.0
ENV PORT=4321

USER node

EXPOSE 4321

CMD ["node", "./dist/server/entry.mjs"]
"#,
            node_version = node_version,
            lock_copy = lock_copy,
            install_cmd = install_cmd,
            build_cmd = build_cmd,
        )
    } else {
        // Static (SSG) → nginx
        format!(
            r#"# ── Build ──
FROM node:{node_version}-alpine AS builder
WORKDIR /app
{lock_copy}
RUN {install_cmd}
COPY . .
RUN {build_cmd}

# ── Runtime ──
FROM nginx:alpine

COPY --from=builder /app/dist /usr/share/nginx/html

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
"#,
            node_version = node_version,
            lock_copy = lock_copy,
            install_cmd = install_cmd,
            build_cmd = build_cmd,
        )
    }
}

/// Next.js → standalone mode with non-root user
/// Reference: https://github.com/vercel/next.js/blob/canary/examples/with-docker/Dockerfile
fn next_dockerfile(path: &Path, node_version: &str, pkg_manager: &str) -> String {
    let (install_cmd, lock_copy) = pkg_install_cmds(pkg_manager);
    let build_cmd = detect_safe_build_cmd(path, "next");

    format!(
        r#"# ── Dependencies ──
FROM node:{node_version}-alpine AS deps
WORKDIR /app
{lock_copy}
RUN {install_cmd}

# ── Build ──
FROM node:{node_version}-alpine AS builder
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
ENV NODE_ENV=production
RUN {build_cmd}

# ── Runtime ──
FROM node:{node_version}-alpine

WORKDIR /app

ENV NODE_ENV=production
ENV PORT=3000
ENV HOSTNAME="0.0.0.0"

COPY --from=builder /app/public ./public
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static

USER node

EXPOSE 3000

CMD ["node", "server.js"]
"#,
        node_version = node_version,
        lock_copy = lock_copy,
        install_cmd = install_cmd,
        build_cmd = build_cmd,
    )
}

/// Vite / React / Vue SPA → multi-stage build + nginx
fn vite_dockerfile(path: &Path, node_version: &str, pkg_manager: &str) -> String {
    let (install_cmd, lock_copy) = pkg_install_cmds(pkg_manager);
    let build_cmd = detect_safe_build_cmd(path, "vite");

    format!(
        r#"# ── Build ──
FROM node:{node_version}-alpine AS builder
WORKDIR /app
{lock_copy}
RUN {install_cmd}
COPY . .
RUN {build_cmd}

# ── Runtime ──
FROM nginx:alpine

COPY --from=builder /app/dist /usr/share/nginx/html

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
"#,
        node_version = node_version,
        lock_copy = lock_copy,
        install_cmd = install_cmd,
        build_cmd = build_cmd,
    )
}

/// Generic Node.js server (Express, Fastify, etc.)
fn generic_node_dockerfile(_path: &Path, node_version: &str, pkg_manager: &str) -> String {
    let (_install_cmd, lock_copy) = pkg_install_cmds(pkg_manager);
    let prod_install = match pkg_manager {
        "pnpm" => "pnpm install --frozen-lockfile --prod",
        "yarn" => "yarn install --frozen-lockfile --production",
        _ => "npm ci --omit=dev",
    };

    format!(
        r#"# ── Runtime ──
FROM node:{node_version}-alpine

WORKDIR /app

{lock_copy}
RUN {prod_install}

COPY . .

USER node

EXPOSE 3000

CMD ["node", "server.js"]
"#,
        node_version = node_version,
        lock_copy = lock_copy,
        prod_install = prod_install,
    )
}

/// Returns (install_cmd, lock_file_copy_lines) for the detected package manager.
fn pkg_install_cmds(pkg_manager: &str) -> (&'static str, String) {
    match pkg_manager {
        "pnpm" => (
            "corepack enable pnpm && pnpm install --frozen-lockfile",
            "COPY package.json pnpm-lock.yaml ./".to_string(),
        ),
        "yarn" => (
            "corepack enable yarn && yarn install --frozen-lockfile",
            "COPY package.json yarn.lock ./".to_string(),
        ),
        _ => (
            "npm ci",
            "COPY package.json package-lock.json* ./".to_string(),
        ),
    }
}

/// Detect if `npm run build` would invoke `tsc` which can fail on strict checks.
/// In Docker builds we want production artifacts, not type-checking — that belongs in CI.
fn detect_safe_build_cmd(path: &Path, framework: &str) -> String {
    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(build_script) = pkg
                .get("scripts")
                .and_then(|s| s.get("build"))
                .and_then(|b| b.as_str())
            {
                if build_script.contains("tsc") {
                    return match framework {
                        "vite" | "react" => "npx vite build".to_string(),
                        "next" => "npx next build".to_string(),
                        "astro" => "npx astro build".to_string(),
                        _ => "npm run build".to_string(),
                    };
                }
            }
        }
    }

    match framework {
        "next" | "vite" | "react" => "npm run build".to_string(),
        "astro" => "npx astro build".to_string(),
        _ => String::new(),
    }
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
    "22".to_string()
}

fn detect_node_framework(path: &Path) -> String {
    // Config file detection (most reliable)
    if path.join("astro.config.mjs").exists()
        || path.join("astro.config.ts").exists()
        || path.join("astro.config.js").exists()
    {
        return "astro".to_string();
    }
    if path.join("next.config.js").exists()
        || path.join("next.config.mjs").exists()
        || path.join("next.config.ts").exists()
    {
        return "next".to_string();
    }
    if path.join("vite.config.ts").exists()
        || path.join("vite.config.js").exists()
        || path.join("vite.config.mjs").exists()
    {
        return "vite".to_string();
    }

    // Fallback: package.json deps
    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        let lower = content.to_lowercase();
        if lower.contains("\"astro\"") { return "astro".to_string(); }
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

/// Detect if Astro project uses SSR (server output) or SSG (static).
fn detect_astro_ssr(path: &Path) -> bool {
    for config_file in &["astro.config.mjs", "astro.config.ts", "astro.config.js"] {
        if let Ok(content) = std::fs::read_to_string(path.join(config_file)) {
            // output: 'server' or output: "server"
            if content.contains("output:") && (content.contains("'server'") || content.contains("\"server\"")) {
                return true;
            }
        }
    }
    false
}

// ── Rust ──
// Best practices: cargo-chef for dep caching, distroless/slim runtime, non-root

fn rust_dockerfile(path: &Path) -> String {
    let bin_name = detect_rust_bin_name(path);

    format!(
        r#"# ── Chef stage (dependency caching) ──
FROM rust:1.83-slim AS chef
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

COPY . .
RUN cargo build --release --bin {bin_name}

# ── Runtime stage ──
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app && useradd -r -g app app

WORKDIR /app
COPY --from=builder /app/target/release/{bin_name} .

USER app

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
// Best practices: distroless runtime, static binary, non-root

fn go_dockerfile(path: &Path) -> String {
    let _module_name = detect_go_module(path);

    r#"# ── Build stage ──
FROM golang:1.22-alpine AS builder

WORKDIR /app

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o /app/server .

# ── Runtime stage ──
FROM gcr.io/distroless/static-debian12

WORKDIR /app
COPY --from=builder /app/server .

USER nonroot:nonroot

EXPOSE 8080

CMD ["/app/server"]
"#.to_string()
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
        assert!(result.contains("USER app"));
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
        assert!(result.contains("EXPOSE 80"));
    }

    #[test]
    fn test_generate_node_vite_with_tsc_bypass() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"my-app","scripts":{"build":"tsc && vite build"},"dependencies":{"vite":"^5.0.0","react":"^18"}}"#,
        ).unwrap();

        let result = generate(dir.path(), ProjectType::Node).unwrap();
        assert!(result.contains("npx vite build"), "Should bypass tsc and call vite directly");
        assert!(!result.contains("npm run build"), "Should NOT use npm run build when tsc is present");
        assert!(result.contains("nginx"));
    }

    #[test]
    fn test_generate_node_astro_ssg() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"my-landing","dependencies":{"astro":"^4.0.0"}}"#,
        ).unwrap();
        std::fs::write(dir.path().join("astro.config.mjs"), "export default {}").unwrap();

        let result = generate(dir.path(), ProjectType::Node).unwrap();
        assert!(result.contains("astro build"), "Should use astro build");
        assert!(result.contains("nginx"), "Astro SSG should use nginx");
        assert!(result.contains("EXPOSE 80"), "Static should expose 80");
    }

    #[test]
    fn test_generate_node_astro_ssr() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"my-app","dependencies":{"astro":"^4.0.0","@astrojs/node":"^8.0.0"}}"#,
        ).unwrap();
        std::fs::write(
            dir.path().join("astro.config.mjs"),
            "export default { output: 'server' }",
        ).unwrap();

        let result = generate(dir.path(), ProjectType::Node).unwrap();
        assert!(result.contains("astro build"), "Should use astro build");
        assert!(result.contains("entry.mjs"), "SSR should use entry.mjs");
        assert!(result.contains("EXPOSE 4321"), "SSR should expose 4321");
        assert!(result.contains("USER node"), "Should use non-root user");
    }

    #[test]
    fn test_generate_node_next() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"my-next","dependencies":{"next":"^14.0.0","react":"^18"}}"#,
        ).unwrap();
        std::fs::write(dir.path().join("next.config.js"), "module.exports = {}").unwrap();

        let result = generate(dir.path(), ProjectType::Node).unwrap();
        assert!(result.contains("next"), "Should detect Next.js");
        assert!(result.contains("standalone"), "Should use standalone mode");
        assert!(result.contains("USER node"), "Should use non-root user");
        assert!(result.contains("EXPOSE 3000"));
    }

    #[test]
    fn test_generate_node_pnpm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"my-app","dependencies":{"vite":"^5.0.0"}}"#,
        ).unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();

        let result = generate(dir.path(), ProjectType::Node).unwrap();
        assert!(result.contains("pnpm"), "Should detect pnpm");
        assert!(result.contains("corepack enable pnpm"), "Should enable corepack");
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
        assert!(result.contains("USER app"));
    }

    #[test]
    fn test_generate_go() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module github.com/user/myapp\n\ngo 1.22\n").unwrap();

        let result = generate(dir.path(), ProjectType::Go).unwrap();
        assert!(result.contains("golang:1.22"));
        assert!(result.contains("distroless"));
        assert!(result.contains("USER nonroot"));
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

    #[test]
    fn test_generate_dockerignore_node() {
        let content = generate_dockerignore(ProjectType::Node);
        assert!(content.contains("node_modules"));
        assert!(content.contains(".git"));
    }

    #[test]
    fn test_detect_framework_by_config_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("next.config.mjs"), "").unwrap();

        assert_eq!(detect_node_framework(dir.path()), "next");
    }

    #[test]
    fn test_detect_framework_vite_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("vite.config.ts"), "").unwrap();

        assert_eq!(detect_node_framework(dir.path()), "vite");
    }
}
