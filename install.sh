#!/usr/bin/env bash
# Void Stack installer — https://www.void-stack.dev
# Usage:
#   curl -fsSL https://www.void-stack.dev/install.sh | bash
#   curl -fsSL https://www.void-stack.dev/install.sh | bash -s -- --no-mcp

set -euo pipefail

REPO="Mague/void-stack"
INSTALL_DIR="${VOID_INSTALL_DIR:-$HOME/.local/bin}"
BINARIES=("void" "void-stack-tui" "void-stack-mcp" "void-stack-daemon")
AUTO_MCP=true

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; DIM='\033[2m'; RESET='\033[0m'

info()    { echo -e "${CYAN}info${RESET}  $*"; }
success() { echo -e "${GREEN}✓${RESET}     $*"; }
warn()    { echo -e "${YELLOW}warn${RESET}  $*"; }
step()    { echo -e "\n${BOLD}$*${RESET}"; }
error()   { echo -e "${RED}error${RESET} $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin)          BINARIES=("$2"); shift 2 ;;
    --install-dir)  INSTALL_DIR="$2"; shift 2 ;;
    --no-mcp)       AUTO_MCP=false; shift ;;
    --help|-h)
      echo "Usage: install.sh [--bin <name>] [--install-dir <path>] [--no-mcp]"
      exit 0 ;;
    *) error "Unknown argument: $1" ;;
  esac
done

OS="$(uname -s)"; ARCH="$(uname -m)"
case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      *)       error "Unsupported arch: $ARCH" ;;
    esac ;;
  Darwin)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-apple-darwin" ;;
      arm64)   TARGET="aarch64-apple-darwin" ;;
      *)       error "Unsupported arch: $ARCH" ;;
    esac ;;
  *) error "Unsupported OS: $OS. Use install.ps1 on Windows." ;;
esac

for cmd in curl tar; do
  command -v "$cmd" &>/dev/null || error "'$cmd' not found."
done

step "📦 Void Stack Installer"
info "Fetching latest version..."
VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)".*/\1/' | head -1)
[[ -z "$VERSION" ]] && error "Could not fetch version. Check your connection."
info "Installing Void Stack ${BOLD}${VERSION}${RESET} → ${INSTALL_DIR}"

ASSET="void-stack-${VERSION}-${TARGET}.tar.gz"
TMP_DIR="$(mktemp -d)"; trap 'rm -rf "$TMP_DIR"' EXIT
info "Downloading ${ASSET}..."
curl -fsSL --progress-bar \
  "https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}" \
  -o "${TMP_DIR}/${ASSET}" || error "Download failed."
tar -xzf "${TMP_DIR}/${ASSET}" -C "$TMP_DIR"
EXTRACTED="${TMP_DIR}/void-stack-${VERSION}-${TARGET}"

step "🔧 Installing binaries"
mkdir -p "$INSTALL_DIR"
MCP_BIN=""
for bin in "${BINARIES[@]}"; do
  SRC="${EXTRACTED}/${bin}"
  if [[ -f "$SRC" ]]; then
    chmod +x "$SRC"; cp "$SRC" "${INSTALL_DIR}/${bin}"
    success "Installed: ${INSTALL_DIR}/${bin}"
    [[ "$bin" == "void-stack-mcp" ]] && MCP_BIN="${INSTALL_DIR}/void-stack-mcp"
  else
    warn "${bin} not found in release"
  fi
done

if [[ "$OS" == "Darwin" ]]; then
  for bin in "${BINARIES[@]}"; do
    BIN_PATH="${INSTALL_DIR}/${bin}"
    [[ -f "$BIN_PATH" ]] && xattr -d com.apple.quarantine "$BIN_PATH" 2>/dev/null || true
  done
fi

if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
  warn "${INSTALL_DIR} not in PATH. Add to ~/.bashrc or ~/.zshrc:"
  echo -e "  ${BOLD}export PATH=\"\$PATH:${INSTALL_DIR}\"${RESET}"
fi

# ── MCP auto-config ───────────────────────────────────────────────────────────
merge_standard() {
  local file="$1" key="$2"
  [[ ! -f "$file" ]] && return 1
  cp "$file" "${file}.void-stack-backup"
  command -v python3 &>/dev/null || return 1
  python3 - "$file" "$key" "$MCP_BIN" <<'PY'
import json, sys
f, key, bin = sys.argv[1], sys.argv[2], sys.argv[3]
with open(f) as fh:
  try: d = json.load(fh)
  except: d = {}
if key not in d: d[key] = {}
if 'void-stack' not in d[key]:
  d[key]['void-stack'] = {'command': bin}
with open(f, 'w') as fh: json.dump(d, fh, indent=2)
PY
}

merge_opencode() {
  local file="$1"
  [[ ! -f "$file" ]] && return 1
  cp "$file" "${file}.void-stack-backup"
  command -v python3 &>/dev/null || return 1
  python3 - "$file" "$MCP_BIN" <<'PY'
import json, sys
f, bin = sys.argv[1], sys.argv[2]
with open(f) as fh:
  try: d = json.load(fh)
  except: d = {}
if 'mcp' not in d: d['mcp'] = {}
if 'void-stack' not in d['mcp']:
  d['mcp']['void-stack'] = {'type': 'local', 'command': [bin], 'enabled': True}
with open(f, 'w') as fh: json.dump(d, fh, indent=2)
PY
}

merge_zed() {
  local file="$1"
  [[ ! -f "$file" ]] && return 1
  cp "$file" "${file}.void-stack-backup"
  command -v python3 &>/dev/null || return 1
  python3 - "$file" "$MCP_BIN" <<'PY'
import json, sys
f, bin = sys.argv[1], sys.argv[2]
with open(f) as fh:
  try: d = json.load(fh)
  except: d = {}
if 'context_servers' not in d: d['context_servers'] = {}
if 'void-stack' not in d['context_servers']:
  d['context_servers']['void-stack'] = {'command': {'path': bin, 'args': []}}
with open(f, 'w') as fh: json.dump(d, fh, indent=2)
PY
}

if [[ "$AUTO_MCP" == "true" && -n "$MCP_BIN" ]]; then
  step "🤖 Detecting MCP-compatible tools..."

  declare -a FOUND_NAMES FOUND_FILES FOUND_TYPES

  add_if_exists() {
    local name="$1" file="$2" type="$3"
    [[ -f "$file" ]] && FOUND_NAMES+=("$name") && FOUND_FILES+=("$file") && FOUND_TYPES+=("$type")
  }

  case "$OS" in
    Darwin)
      add_if_exists "Claude Desktop" "$HOME/Library/Application Support/Claude/claude_desktop_config.json" "standard"
      add_if_exists "Cursor"         "$HOME/.cursor/mcp.json" "standard"
      add_if_exists "Windsurf"       "$HOME/.codeium/windsurf/mcp_server_config.json" "standard"
      add_if_exists "OpenCode"       "$HOME/.config/opencode/opencode.json" "opencode"
      add_if_exists "Cline"          "$HOME/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json" "standard"
      add_if_exists "Continue.dev"   "$HOME/.continue/config.json" "standard"
      add_if_exists "Zed"            "$HOME/Library/Application Support/Zed/settings.json" "zed"
      ;;
    Linux)
      add_if_exists "Claude Desktop" "$HOME/.config/Claude/claude_desktop_config.json" "standard"
      add_if_exists "Cursor"         "$HOME/.cursor/mcp.json" "standard"
      add_if_exists "Windsurf"       "$HOME/.codeium/windsurf/mcp_server_config.json" "standard"
      add_if_exists "OpenCode"       "$HOME/.config/opencode/opencode.json" "opencode"
      add_if_exists "Cline"          "$HOME/.config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json" "standard"
      add_if_exists "Continue.dev"   "$HOME/.continue/config.json" "standard"
      add_if_exists "Zed"            "$HOME/.config/zed/settings.json" "zed"
      ;;
  esac

  # Claude Code CLI
  HAS_CLAUDE_CODE=false
  command -v claude &>/dev/null && HAS_CLAUDE_CODE=true

  if [[ ${#FOUND_NAMES[@]} -eq 0 && "$HAS_CLAUDE_CODE" == "false" ]]; then
    info "No MCP tools detected. Configure manually later."
  else
    echo ""
    echo -e "${BOLD}MCP tools detected:${RESET}"
    for i in "${!FOUND_NAMES[@]}"; do
      echo -e "  ${GREEN}✓${RESET} ${FOUND_NAMES[$i]} ${DIM}(${FOUND_FILES[$i]})${RESET}"
    done
    [[ "$HAS_CLAUDE_CODE" == "true" ]] && \
      echo -e "  ${GREEN}✓${RESET} Claude Code ${DIM}(claude mcp add)${RESET}"
    echo ""
    read -r -p "Auto-configure void-stack-mcp in all detected tools? [Y/n] " CONFIRM
    CONFIRM="${CONFIRM:-Y}"

    if [[ "$CONFIRM" =~ ^[Yy]$ ]]; then
      step "⚙️  Configuring..."
      for i in "${!FOUND_NAMES[@]}"; do
        case "${FOUND_TYPES[$i]}" in
          standard) merge_standard "${FOUND_FILES[$i]}" "mcpServers" \
            && success "${FOUND_NAMES[$i]} configured" \
            || warn "Could not configure ${FOUND_NAMES[$i]}" ;;
          opencode) merge_opencode "${FOUND_FILES[$i]}" \
            && success "${FOUND_NAMES[$i]} configured" \
            || warn "Could not configure ${FOUND_NAMES[$i]}" ;;
          zed)      merge_zed "${FOUND_FILES[$i]}" \
            && success "${FOUND_NAMES[$i]} configured" \
            || warn "Could not configure ${FOUND_NAMES[$i]}" ;;
        esac
        echo -e "  ${DIM}Backup: ${FOUND_FILES[$i]}.void-stack-backup${RESET}"
      done

      if [[ "$HAS_CLAUDE_CODE" == "true" ]]; then
        if claude mcp list 2>/dev/null | grep -q "void-stack"; then
          info "Claude Code: void-stack already configured"
        else
          claude mcp add void-stack "$MCP_BIN" 2>/dev/null \
            && success "Claude Code configured" \
            || warn "Could not configure Claude Code automatically"
        fi
      fi

      echo ""
      warn "Restart detected apps to load the new MCP server."
    else
      info "Skipping MCP config. Configure manually with:"
      echo -e "  ${DIM}{ \"mcpServers\": { \"void-stack\": { \"command\": \"${MCP_BIN}\" } } }${RESET}"
    fi
  fi
fi

echo ""
echo -e "${GREEN}${BOLD}Done!${RESET} Void Stack ${VERSION} installed."
echo ""
echo -e "  ${BOLD}void add my-project ~/projects/my-app${RESET}"
echo -e "  ${BOLD}void start my-project${RESET}"
echo ""