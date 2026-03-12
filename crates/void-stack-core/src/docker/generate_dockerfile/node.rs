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

pub(super) fn detect_node_version(path: &Path) -> String {
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
        if lower.contains("\"astro\"") { return "astro".to_string(); }
        if lower.contains("\"next\"") { return "next".to_string(); }
        if lower.contains("\"vite\"") { return "vite".to_string(); }
        if lower.contains("\"react-scripts\"") { return "react".to_string(); }
        if lower.contains("\"express\"") { return "express".to_string(); }
    }
    "generic".to_string()
}

pub(super) fn detect_node_pkg_manager(path: &Path) -> String {
    if path.join("pnpm-lock.yaml").exists() { return "pnpm".to_string(); }
    if path.join("yarn.lock").exists() { return "yarn".to_string(); }
    "npm".to_string()
}

fn detect_astro_ssr(path: &Path) -> bool {
    for config_file in &["astro.config.mjs", "astro.config.ts", "astro.config.js"] {
        if let Ok(content) = std::fs::read_to_string(path.join(config_file)) {
            if content.contains("output:") && (content.contains("'server'") || content.contains("\"server\"")) {
                return true;
            }
        }
    }
    false
}
