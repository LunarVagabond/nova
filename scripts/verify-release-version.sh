#!/usr/bin/env bash
# Verify Nova's three version-bearing files agree with the semver parsed
# from a release tag:
#   - Cargo.toml            [workspace.package].version (all three crates
#                            inherit it via `version.workspace = true`)
#   - crates/nova-app/package.json
#   - crates/nova-app/src-tauri/tauri.conf.json
#
# Run by CI right after checkout, before any bundle build starts, so a
# stale/forgotten version bump fails fast instead of shipping a release
# whose packages disagree about their own version.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

TAG="${1:?usage: verify-release-version.sh <tag>}"
if [[ ! "${TAG#v}" =~ ^([0-9]+\.[0-9]+\.[0-9]+) ]]; then
  echo "Could not derive semantic version from tag: ${TAG}"
  exit 1
fi
VERSION="${BASH_REMATCH[1]}"

check_file() {
  local file="$1"
  local actual="$2"
  if [ "$actual" != "$VERSION" ]; then
    echo "Version mismatch in $file: expected $VERSION, got '$actual'"
    exit 1
  fi
}

cargo_ver="$(sed -n '/^\[workspace\.package\]/,/^\[/p' Cargo.toml \
  | sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' | head -n1)"
pkg_ver="$(node -p "require('./crates/nova-app/package.json').version")"
tauri_ver="$(node -p "JSON.parse(require('fs').readFileSync('./crates/nova-app/src-tauri/tauri.conf.json','utf8')).version")"

check_file "Cargo.toml" "$cargo_ver"
check_file "crates/nova-app/package.json" "$pkg_ver"
check_file "crates/nova-app/src-tauri/tauri.conf.json" "$tauri_ver"

echo "Version files match tag ${TAG} (${VERSION})"
