#!/usr/bin/env bash
# Void Stack installer — https://void-stack.dev
# Usage:
#   curl -fsSL https://void-stack.dev/install.sh | bash
#   curl -fsSL https://void-stack.dev/install.sh | bash -s -- --bin void          # solo CLI
#   curl -fsSL https://void-stack.dev/install.sh | bash -s -- --install-dir ~/.local/bin

set -euo pipefail

REPO="Mague/void-stack"
INSTALL_DIR="${VOID_INSTALL_DIR:-$HOME/.local/bin}"
BINARIES=("void" "void-stack-tui" "void-stack-mcp" "void-stack-daemon")

# ── Colores ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

info()    { echo -e "${CYAN}info${RESET}  $*"; }
success() { echo -e "${GREEN}✓${RESET}     $*"; }
warn()    { echo -e "${YELLOW}warn${RESET}  $*"; }
error()   { echo -e "${RED}error${RESET} $*" >&2; exit 1; }

# ── Parse args ───────────────────────────────────────────────────────────────
SELECTED_BIN=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin)           SELECTED_BIN="$2"; shift 2 ;;
    --install-dir)   INSTALL_DIR="$2";  shift 2 ;;
    --help|-h)
      echo "Usage: install.sh [--bin <name>] [--install-dir <path>]"
      echo ""
      echo "Binaries: void, void-stack-tui, void-stack-mcp, void-stack-daemon"
      echo "Default install dir: ~/.local/bin  (override: VOID_INSTALL_DIR)"
      exit 0
      ;;
    *) error "Unknown argument: $1" ;;
  esac
done

[[ -n "$SELECTED_BIN" ]] && BINARIES=("$SELECTED_BIN")

# ── Detectar plataforma ───────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      *)       error "Arquitectura no soportada: $ARCH" ;;
    esac
    EXT="tar.gz"
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-apple-darwin" ;;
      arm64)   TARGET="aarch64-apple-darwin" ;;
      *)       error "Arquitectura no soportada: $ARCH" ;;
    esac
    EXT="tar.gz"
    ;;
  *)
    error "Sistema operativo no soportado: $OS. Usa install.ps1 en Windows."
    ;;
esac

# ── Verificar dependencias del script ────────────────────────────────────────
for cmd in curl tar; do
  command -v "$cmd" &>/dev/null || error "'$cmd' no encontrado. Instálalo primero."
done

# ── Obtener última versión ────────────────────────────────────────────────────
info "Buscando última versión..."
API_URL="https://api.github.com/repos/${REPO}/releases/latest"
VERSION=$(curl -fsSL "$API_URL" | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)".*/\1/' | head -1)
[[ -z "$VERSION" ]] && error "No se pudo obtener la versión. Verifica tu conexión."

info "Instalando Void Stack ${BOLD}${VERSION}${RESET} → ${INSTALL_DIR}"

# ── Descargar y extraer ───────────────────────────────────────────────────────
ASSET="void-stack-${VERSION}-${TARGET}.${EXT}"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

info "Descargando ${ASSET}..."
curl -fsSL --progress-bar "$DOWNLOAD_URL" -o "${TMP_DIR}/${ASSET}" \
  || error "Descarga fallida: ${DOWNLOAD_URL}"

tar -xzf "${TMP_DIR}/${ASSET}" -C "$TMP_DIR"
EXTRACTED_DIR="${TMP_DIR}/void-stack-${VERSION}-${TARGET}"

# ── Instalar binarios ─────────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"

for bin in "${BINARIES[@]}"; do
  SRC="${EXTRACTED_DIR}/${bin}"
  if [[ -f "$SRC" ]]; then
    chmod +x "$SRC"
    cp "$SRC" "${INSTALL_DIR}/${bin}"
    success "Instalado: ${INSTALL_DIR}/${bin}"
  else
    warn "${bin} no encontrado en el release (puede no estar disponible para ${TARGET})"
  fi
done

# ── Verificar PATH ────────────────────────────────────────────────────────────
echo ""
if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
  warn "${INSTALL_DIR} no está en tu PATH."
  echo ""
  echo "  Agrega esto a tu ~/.bashrc, ~/.zshrc o ~/.profile:"
  echo ""
  echo -e "  ${BOLD}export PATH=\"\$PATH:${INSTALL_DIR}\"${RESET}"
  echo ""
else
  echo -e "${GREEN}${BOLD}¡Listo!${RESET} Void Stack ${VERSION} instalado correctamente."
  echo ""
  echo "  Empieza con:"
  echo -e "  ${BOLD}void add mi-proyecto /ruta/al/proyecto${RESET}"
  echo -e "  ${BOLD}void start mi-proyecto${RESET}"
fi

# ── macOS: quitar cuarentena ──────────────────────────────────────────────────
if [[ "$OS" == "Darwin" ]]; then
  for bin in "${BINARIES[@]}"; do
    BIN_PATH="${INSTALL_DIR}/${bin}"
    [[ -f "$BIN_PATH" ]] && xattr -d com.apple.quarantine "$BIN_PATH" 2>/dev/null || true
  done
fi