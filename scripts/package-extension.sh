#!/usr/bin/env bash
# Package void-stack-mcp as a Claude Desktop Extension (.mcpb)
# Usage: ./scripts/package-extension.sh <platform> <binary_path> <version>
# Example: ./scripts/package-extension.sh windows-x64 ./bins/void-stack-mcp.exe 0.24.0

set -euo pipefail

PLATFORM="${1:?Usage: $0 <platform> <binary_path> <version>}"
BINARY="${2:?Missing binary path}"
VERSION="${3:?Missing version}"

STAGING="staging-extension"
OUTPUT="void-stack-${VERSION}-${PLATFORM}.mcpb"

rm -rf "$STAGING"
mkdir -p "$STAGING"

if [[ "$PLATFORM" == windows* ]]; then
  cp "$BINARY" "$STAGING/void-stack-mcp.exe"
  # Adjust command in manifest for Windows
  sed 's|void-stack-mcp"|void-stack-mcp.exe"|g' manifest.json > "$STAGING/manifest.json"
else
  cp "$BINARY" "$STAGING/void-stack-mcp"
  chmod +x "$STAGING/void-stack-mcp"
  cp manifest.json "$STAGING/manifest.json"
fi

# Stamp version
sed -i.bak "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" \
  "$STAGING/manifest.json"
rm -f "$STAGING/manifest.json.bak"

cd "$STAGING"
zip -r "../$OUTPUT" .
cd ..

rm -rf "$STAGING"
echo "Packaged: $OUTPUT"
