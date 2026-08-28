#!/usr/bin/env bash
# Stage the three built bundle combinations into release-files/ with
# predictable, versioned names for a GitHub Release upload.
#
# Expects scripts/build-release-bundles.sh to have already run. Fails loudly
# (rather than silently uploading a partial release) if any expected
# artifact is missing.
#
# Naming scheme (all Linux x86_64 for now -- see docs/project-management/
# release-strategy.md):
#   nova-cli-v<ver>-linux-x86_64.tar.gz   Engine+CLI, binary archive
#   nova-cli_<ver>_amd64.deb              Engine+CLI, .deb
#   nova-gui_<ver>_amd64.AppImage         Engine+GUI
#   nova-gui_<ver>_amd64.deb              Engine+GUI
#   nova-full_<ver>_amd64.AppImage        Engine+CLI+GUI
#   nova-full_<ver>_amd64.deb             Engine+CLI+GUI
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

VERSION="${1:?usage: stage-release.sh <version> [out_dir]}"
OUT_DIR="${2:-release-files}"

TARGET_RELEASE="target/release"
BUNDLE="$TARGET_RELEASE/bundle"
# Engine+CLI+GUI is built with its own CARGO_TARGET_DIR (see
# scripts/build-release-bundles.sh) so its AppImage build doesn't wipe out
# Engine+GUI's -- Tauri's AppImage bundler clears the whole bundle/appimage
# directory on every build regardless of productName.
FULL_TARGET_RELEASE="target/full-bundle/release"
FULL_BUNDLE="$FULL_TARGET_RELEASE/bundle"

mkdir -p "$OUT_DIR"
shopt -s nullglob

fail() {
  echo "stage-release: $1" >&2
  exit 1
}

require_glob() {
  # Prints the first match of a glob pattern, or fails loudly if there are
  # none. Takes the glob as literal args (already expanded by the caller)
  # so it works whether nullglob left zero, one, or several matches.
  local desc="$1"
  shift
  if [ "$#" -eq 0 ]; then
    fail "no $desc found"
  fi
  if [ "$#" -gt 1 ]; then
    echo "stage-release: multiple $desc found, using the first: $1" >&2
  fi
  printf '%s' "$1"
}

echo "stage-release: staging version $VERSION into $OUT_DIR"

# --- Engine+CLI ---------------------------------------------------------
cli_bin="$TARGET_RELEASE/nova"
cli_deb="$TARGET_RELEASE/nova-cli.deb"
[ -f "$cli_bin" ] || fail "missing CLI binary: $cli_bin (run scripts/build-release-bundles.sh first)"
[ -f "$cli_deb" ] || fail "missing CLI .deb: $cli_deb (run scripts/build-release-bundles.sh first)"

tar -czf "$OUT_DIR/nova-cli-v${VERSION}-linux-x86_64.tar.gz" -C "$TARGET_RELEASE" nova
cp -f "$cli_deb" "$OUT_DIR/nova-cli_${VERSION}_amd64.deb"

# --- Engine+GUI (productName "nova-app") --------------------------------
gui_appimage="$(require_glob "Engine+GUI AppImage" "$BUNDLE"/appimage/nova-app_*.AppImage)"
gui_deb="$(require_glob "Engine+GUI .deb" "$BUNDLE"/deb/nova-app_*.deb)"
cp -f "$gui_appimage" "$OUT_DIR/nova-gui_${VERSION}_amd64.AppImage"
cp -f "$gui_deb" "$OUT_DIR/nova-gui_${VERSION}_amd64.deb"

# --- Engine+CLI+GUI (productName "nova-full", separate target dir) -----
full_appimage="$(require_glob "Engine+CLI+GUI AppImage" "$FULL_BUNDLE"/appimage/nova-full_*.AppImage)"
full_deb="$(require_glob "Engine+CLI+GUI .deb" "$FULL_BUNDLE"/deb/nova-full_*.deb)"
cp -f "$full_appimage" "$OUT_DIR/nova-full_${VERSION}_amd64.AppImage"
cp -f "$full_deb" "$OUT_DIR/nova-full_${VERSION}_amd64.deb"

echo "stage-release: staged files:"
ls -la "$OUT_DIR"
