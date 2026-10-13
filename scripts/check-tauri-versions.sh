#!/usr/bin/env bash
# `cargo tauri build` refuses to build when a Tauri crate and its npm package
# are on different major.minor releases. Dependabot updates Cargo.lock and
# package-lock.json in separate PRs, so check the pair in CI before release.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
lock="$root/Cargo.lock"
npm_lock="$root/crates/void-stack-desktop/frontend/package-lock.json"

crate_version() {
  awk -v name="$1" '$0 == "name = \"" name "\"" { getline; gsub(/version = |"/, ""); print; exit }' "$lock"
}
npm_version() {
  python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['packages']['node_modules/' + sys.argv[2]]['version'])" "$npm_lock" "$1"
}

status=0
for pair in "tauri:@tauri-apps/api" "tauri-plugin-dialog:@tauri-apps/plugin-dialog" "tauri-plugin-opener:@tauri-apps/plugin-opener"; do
  crate="${pair%%:*}"; pkg="${pair#*:}"
  cv="$(crate_version "$crate")"; nv="$(npm_version "$pkg")"
  if [ "${cv%.*}" != "${nv%.*}" ]; then
    echo "MISMATCH $crate $cv vs $pkg $nv (major.minor must match)"
    status=1
  else
    echo "ok $crate $cv / $pkg $nv"
  fi
done
exit $status
