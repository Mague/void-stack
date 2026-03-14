//! Rust Dockerfile generation (cargo-chef pattern).

use std::path::Path;

pub(super) fn rust_dockerfile(path: &Path) -> String {
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
            if trimmed.starts_with("name")
                && let Some(name) = trimmed.split('"').nth(1)
            {
                return name.to_string();
            }
        }
    }
    "app".to_string()
}
