#!/usr/bin/env bash
# Actualiza en local lo mismo que harian los PRs de parches/minors de
# Dependabot, lo prueba todo y lo deja en UN commit en una rama propia.
#
#   bash scripts/update-deps.sh            # rama deps/AAAA-MM desde develop
#   bash scripts/update-deps.sh --here     # sin crear rama (rama actual)
#   JOBS=4 bash scripts/update-deps.sh     # mas hilos (por defecto 2, para
#                                          # no ahogar la PC con el antivirus)
#
# No sube versiones mayores: al final lista las que hay para revisarlas a mano.
set -euo pipefail

JOBS="${JOBS:-2}"
ROOT="$(git rev-parse --show-toplevel)"
FRONT="$ROOT/crates/void-stack-desktop/frontend"
cd "$ROOT"

step() { printf '\n\033[1;36m== %s\033[0m\n' "$*"; }
fail() { printf '\n\033[1;31mX %s\033[0m\n' "$*" >&2; exit 1; }

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  fail "Hay cambios sin commitear: guardalos (git stash) antes de actualizar."
fi

if [ "${1:-}" != "--here" ]; then
  branch="deps/$(date +%Y-%m)"
  step "Rama $branch desde develop"
  git fetch -q origin develop
  git switch -c "$branch" origin/develop
fi

step "cargo update (parches y minors compatibles)"
cargo update 2>&1 | tee /tmp/void-cargo-update.log | grep -E 'Updating|Adding|Removing' || true

step "npm update (rangos de package.json)"
(cd "$FRONT" && npm update --no-audit --no-fund)

step "Tauri: crates y paquetes npm en el mismo major.minor"
bash scripts/check-tauri-versions.sh

step "Rust: fmt + clippy + tests (-j $JOBS)"
cargo fmt --all -- --check
cargo clippy -j "$JOBS" --workspace --all-targets --exclude void-stack-desktop -- -D warnings
cargo test -j "$JOBS" --workspace --exclude void-stack-desktop -- --test-threads="$JOBS"

step "Frontend: lint + build"
(cd "$FRONT" && npm run lint && npm run build)

if git diff --quiet -- Cargo.lock "$FRONT/package-lock.json" "$FRONT/package.json"; then
  step "Nada que actualizar: todo estaba al dia."
  exit 0
fi

step "Commit"
changes="$(grep -E 'Updating' /tmp/void-cargo-update.log | sed 's/^ *Updating /- /' | head -60 || true)"
git add Cargo.lock "$FRONT/package-lock.json" "$FRONT/package.json"
git commit -m "chore(deps): actualizacion de dependencias $(date +%Y-%m)" \
  -m "Parches y minors compatibles (cargo update + npm update). Probado en local: clippy, tests de Rust, lint y build del frontend." \
  -m "${changes:-sin cambios en Cargo.lock}"

step "Versiones mayores pendientes (no se aplican solas)"
(cd "$FRONT" && npm outdated || true)
if cargo outdated --version >/dev/null 2>&1; then
  cargo outdated --workspace --root-deps-only || true
else
  echo "(instala cargo-outdated para ver tambien las de Rust: cargo install cargo-outdated)"
fi

printf '\nListo. Revisa el commit y sube la rama: git push -u origin HEAD\n'
