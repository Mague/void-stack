//! Generate production-grade Dockerfiles based on detected project framework.
//!
//! Templates follow official best practices:
//! - Docker docs: multi-stage builds, layer caching, non-root users
//! - Astro docs: https://docs.astro.build/en/recipes/docker/
//! - Next.js: https://github.com/vercel/next.js/blob/canary/examples/with-docker/Dockerfile
//! - Docker official images: https://github.com/docker-library/official-images

mod python;
mod node;
mod rust_lang;
mod go;
mod flutter;

use std::path::Path;

use crate::model::ProjectType;

/// Generate a Dockerfile for the given project type. Returns None if unsupported.
pub fn generate(project_path: &Path, project_type: ProjectType) -> Option<String> {
    match project_type {
        ProjectType::Python => Some(python::python_dockerfile(project_path)),
        ProjectType::Node => Some(node::node_dockerfile(project_path)),
        ProjectType::Rust => Some(rust_lang::rust_dockerfile(project_path)),
        ProjectType::Go => Some(go::go_dockerfile(project_path)),
        ProjectType::Flutter => Some(flutter::flutter_web_dockerfile(project_path)),
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

        assert_eq!(node::detect_node_framework(dir.path()), "next");
    }

    #[test]
    fn test_detect_framework_vite_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("vite.config.ts"), "").unwrap();

        assert_eq!(node::detect_node_framework(dir.path()), "vite");
    }
}
