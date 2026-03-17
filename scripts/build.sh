#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FRONTEND_DIR="$ROOT_DIR/crates/void-stack-desktop/frontend"

echo "==> Building frontend..."
cd "$FRONTEND_DIR"
npm install --silent
npm run build

echo "==> Running clippy..."
cd "$ROOT_DIR"
cargo clippy --workspace -- -D warnings

echo "==> Building Rust workspace..."
cargo build "$@"

if [[ " $* " == *" --release "* ]]; then
  echo "==> Build complete!  Binary: $ROOT_DIR/target/release/void-stack-desktop"
else
  echo "==> Build complete!  Binary: $ROOT_DIR/target/debug/void-stack-desktop"
fi
