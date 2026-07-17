//! Node.js Dockerfile generation (Astro, Next.js, Vite, Express).

use std::path::Path;

pub(super) fn node_dockerfile(path: &Path) -> String {
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

fn astro_dockerfile(path: &Path, node_version: &str, pkg_manager: &str) -> String {
    let (install_cmd, lock_copy) = pkg_install_cmds(pkg_manager);
    let build_cmd = detect_safe_build_cmd(path, "astro");
    let is_ssr = detect_astro_ssr(path);

    if is_ssr {
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

fn detect_safe_build_cmd(path: &Path, framework: &str) -> String {
    if let Ok(content) = std::fs::read_to_string(path.join("package.json"))
        && let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(build_script) = pkg
            .get("scripts")
            .and_then(|s| s.get("build"))
            .and_then(|b| b.as_str())
        && build_script.contains("tsc")
    {
        return match framework {
            "vite" | "react" => "npx vite build".to_string(),
            "next" => "npx next build".to_string(),
            "astro" => "npx astro build".to_string(),
            _ => "npm run build".to_string(),
        };
    }

    match framework {
        "next" | "vite" | "react" => "npm run build".to_string(),
        "astro" => "npx astro build".to_string(),
        _ => String::new(),
    }
}

pub(super) fn detect_node_version(path: &Path) -> String {
    if let Ok(v) = std::fs::read_to_string(path.join(".nvmrc")) {
        let v = v.trim().trim_start_matches('v');
        if !v.is_empty() {
            return v.to_string();
        }
    }
    if let Ok(content) = std::fs::read_to_string(path.join("package.json"))
        && let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(engines) = pkg
            .get("engines")
            .and_then(|e| e.get("node"))
            .and_then(|n| n.as_str())
    {
        let clean = engines.trim_start_matches(['>', '=', '<', '~', '^']);
        if let Some(major) = clean.split('.').next()
            && major.parse::<u32>().is_ok()
        {
            return major.to_string();
        }
    }
    "22".to_string()
}

pub(super) fn detect_node_framework(path: &Path) -> String {
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

    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        let lower = content.to_lowercase();
        if lower.contains("\"astro\"") {
            return "astro".to_string();
        }
        if lower.contains("\"next\"") {
            return "next".to_string();
        }
        if lower.contains("\"vite\"") {
            return "vite".to_string();
        }
        if lower.contains("\"react-scripts\"") {
            return "react".to_string();
        }
        if lower.contains("\"express\"") {
            return "express".to_string();
        }
    }
    "generic".to_string()
}

pub fn detect_node_pkg_manager(path: &Path) -> String {
    if path.join("pnpm-lock.yaml").exists() {
        return "pnpm".to_string();
    }
    if path.join("yarn.lock").exists() {
        return "yarn".to_string();
    }
    "npm".to_string()
}

fn detect_astro_ssr(path: &Path) -> bool {
    for config_file in &["astro.config.mjs", "astro.config.ts", "astro.config.js"] {
        if let Ok(content) = std::fs::read_to_string(path.join(config_file))
            && content.contains("output:")
            && (content.contains("'server'") || content.contains("\"server\""))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── pkg_install_cmds ──

    #[test]
    fn test_pkg_install_cmds_pnpm() {
        let (install, lock_copy) = pkg_install_cmds("pnpm");
        assert!(
            install.contains("pnpm install --frozen-lockfile"),
            "pnpm should install with a frozen lockfile: {install}"
        );
        assert!(
            lock_copy.contains("pnpm-lock.yaml"),
            "pnpm should copy its own lockfile: {lock_copy}"
        );
    }

    #[test]
    fn test_pkg_install_cmds_yarn() {
        let (install, lock_copy) = pkg_install_cmds("yarn");
        assert!(
            install.contains("yarn install --frozen-lockfile"),
            "yarn should install with a frozen lockfile: {install}"
        );
        assert!(
            lock_copy.contains("yarn.lock"),
            "yarn should copy its own lockfile: {lock_copy}"
        );
    }

    #[test]
    fn test_pkg_install_cmds_defaults_to_npm() {
        // Unknown managers fall back to npm ci
        let (install, lock_copy) = pkg_install_cmds("bun");
        assert_eq!(
            install, "npm ci",
            "unknown manager should default to npm ci"
        );
        assert!(
            lock_copy.contains("package-lock.json"),
            "npm should copy package-lock.json: {lock_copy}"
        );
    }

    // ── detect_node_version ──

    #[test]
    fn test_detect_node_version_strips_v_prefix_from_nvmrc() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".nvmrc"), "v20.11.0\n").unwrap();

        assert_eq!(
            detect_node_version(dir.path()),
            "20.11.0",
            "leading 'v' and whitespace should be stripped"
        );
    }

    #[test]
    fn test_detect_node_version_empty_nvmrc_falls_back_to_default() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".nvmrc"), "  \n").unwrap();

        assert_eq!(
            detect_node_version(dir.path()),
            "22",
            "blank .nvmrc should fall back to the default version"
        );
    }

    #[test]
    fn test_detect_node_version_from_engines_extracts_major() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"engines":{"node":">=18.17.0"}}"#,
        )
        .unwrap();

        assert_eq!(
            detect_node_version(dir.path()),
            "18",
            "engines range should yield the major version only"
        );
    }

    #[test]
    fn test_detect_node_version_non_numeric_engines_falls_back() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"engines":{"node":"lts/*"}}"#,
        )
        .unwrap();

        assert_eq!(
            detect_node_version(dir.path()),
            "22",
            "non-numeric engines value should fall back to the default"
        );
    }

    // ── detect_node_framework ──

    #[test]
    fn test_detect_node_framework_astro_by_config_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("astro.config.ts"), "export default {}").unwrap();

        assert_eq!(
            detect_node_framework(dir.path()),
            "astro",
            "astro.config.ts should win even without package.json"
        );
    }

    #[test]
    fn test_detect_node_framework_astro_by_dependency() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"astro":"^4.0.0"}}"#,
        )
        .unwrap();

        assert_eq!(
            detect_node_framework(dir.path()),
            "astro",
            "astro dependency in package.json should be detected"
        );
    }

    #[test]
    fn test_detect_node_framework_react_scripts() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"react-scripts":"5.0.1"}}"#,
        )
        .unwrap();

        assert_eq!(
            detect_node_framework(dir.path()),
            "react",
            "react-scripts should be classified as react (CRA)"
        );
    }

    #[test]
    fn test_detect_node_framework_generic_without_package_json() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_node_framework(dir.path()),
            "generic",
            "empty directory should be generic"
        );
    }

    // ── detect_node_pkg_manager ──

    #[test]
    fn test_detect_node_pkg_manager_pnpm_wins_over_yarn() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();

        assert_eq!(
            detect_node_pkg_manager(dir.path()),
            "pnpm",
            "pnpm lockfile should take precedence over yarn"
        );
    }

    #[test]
    fn test_detect_node_pkg_manager_defaults_to_npm() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_node_pkg_manager(dir.path()),
            "npm",
            "no lockfile should default to npm"
        );
    }

    // ── detect_safe_build_cmd ──

    #[test]
    fn test_detect_safe_build_cmd_bypasses_tsc_for_next() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"tsc && next build"}}"#,
        )
        .unwrap();

        assert_eq!(
            detect_safe_build_cmd(dir.path(), "next"),
            "npx next build",
            "tsc in the build script should be bypassed"
        );
    }

    #[test]
    fn test_detect_safe_build_cmd_uses_npm_run_build_without_tsc() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"vite build"}}"#,
        )
        .unwrap();

        assert_eq!(
            detect_safe_build_cmd(dir.path(), "vite"),
            "npm run build",
            "plain vite build script should use npm run build"
        );
    }

    #[test]
    fn test_detect_safe_build_cmd_astro_without_package_json() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_safe_build_cmd(dir.path(), "astro"),
            "npx astro build",
            "astro should build via npx when no package.json exists"
        );
    }

    #[test]
    fn test_detect_safe_build_cmd_generic_is_empty() {
        let dir = tempdir().unwrap();
        assert_eq!(
            detect_safe_build_cmd(dir.path(), "express"),
            "",
            "generic frameworks have no build step"
        );
    }

    // ── detect_astro_ssr ──

    #[test]
    fn test_detect_astro_ssr_true_for_server_output() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("astro.config.mjs"),
            "export default { output: 'server' }",
        )
        .unwrap();

        assert!(
            detect_astro_ssr(dir.path()),
            "output: 'server' should be detected as SSR"
        );
    }

    #[test]
    fn test_detect_astro_ssr_false_for_static_output() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("astro.config.mjs"),
            "export default { output: 'static' }",
        )
        .unwrap();

        assert!(
            !detect_astro_ssr(dir.path()),
            "output: 'static' should not be SSR"
        );
    }

    #[test]
    fn test_detect_astro_ssr_false_without_config() {
        let dir = tempdir().unwrap();
        assert!(
            !detect_astro_ssr(dir.path()),
            "missing astro config should default to non-SSR"
        );
    }

    // ── node_dockerfile end-to-end ──

    #[test]
    fn test_node_dockerfile_generic_uses_prod_install() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"srv"}"#).unwrap();

        let dockerfile = node_dockerfile(dir.path());
        assert!(
            dockerfile.contains("FROM node:22-alpine"),
            "default node version should be 22-alpine"
        );
        assert!(
            dockerfile.contains("npm ci --omit=dev"),
            "generic app should install production deps only"
        );
        assert!(
            dockerfile.contains("EXPOSE 3000"),
            "should expose port 3000"
        );
        assert!(
            dockerfile.contains("USER node"),
            "should run as non-root user"
        );
    }

    #[test]
    fn test_node_dockerfile_express_falls_back_to_generic() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"express":"^4.18.0"}}"#,
        )
        .unwrap();

        let dockerfile = node_dockerfile(dir.path());
        assert!(
            dockerfile.contains("CMD [\"node\", \"server.js\"]"),
            "express should use the generic node runtime image"
        );
        assert!(
            !dockerfile.contains("nginx"),
            "express is a server, not a static site"
        );
    }

    #[test]
    fn test_node_dockerfile_astro_ssr_with_pnpm() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"astro":"^4.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("astro.config.mjs"),
            "export default { output: 'server' }",
        )
        .unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();

        let dockerfile = node_dockerfile(dir.path());
        assert!(
            dockerfile.contains("corepack enable pnpm"),
            "pnpm lockfile should enable corepack pnpm"
        );
        assert!(
            dockerfile.contains("entry.mjs"),
            "SSR runtime should launch dist/server/entry.mjs"
        );
        assert!(
            dockerfile.contains("EXPOSE 4321"),
            "astro SSR listens on 4321"
        );
    }

    #[test]
    fn test_node_dockerfile_next_with_yarn() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"next-app"}"#).unwrap();
        std::fs::write(dir.path().join("next.config.ts"), "export default {}").unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();

        let dockerfile = node_dockerfile(dir.path());
        assert!(
            dockerfile.contains("corepack enable yarn"),
            "yarn lockfile should enable corepack yarn"
        );
        assert!(
            dockerfile.contains(".next/standalone"),
            "Next.js should copy the standalone output"
        );
        assert!(
            dockerfile.contains("EXPOSE 3000"),
            "Next.js listens on 3000"
        );
    }

    #[test]
    fn test_node_dockerfile_vite_serves_via_nginx() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"web"}"#).unwrap();
        std::fs::write(dir.path().join("vite.config.js"), "export default {}").unwrap();

        let dockerfile = node_dockerfile(dir.path());
        assert!(
            dockerfile.contains("FROM nginx:alpine"),
            "vite static builds should be served by nginx"
        );
        assert!(dockerfile.contains("EXPOSE 80"), "nginx serves on port 80");
    }
}
