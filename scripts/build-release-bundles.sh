#!/usr/bin/env bash
# Build all three release bundle combinations decided in issue #71:
#
#   1. Engine+CLI      -- release `nova` binary, packaged as a .tar.gz and a
#                         hand-rolled .deb. No AppImage: a headless CLI tool
#                         doesn't benefit from one (a deliberate, reversible
#                         scope decision, not an oversight).
#   2. Engine+GUI      -- the Tauri desktop app, AppImage + .deb only
#                         (tauri.conf.json's bundle.targets is scoped to
#                         ["appimage", "deb"] to match the decision's staged
#                         package-format rollout -- no rpm yet).
#   3. Engine+CLI+GUI  -- the same Tauri app bundle, plus the `nova` CLI
#                         binary placed at usr/bin inside the package, via
#                         tauri.full-bundle.conf.json (see that file for why
#                         `bundle.linux.{deb,appimage}.files` was chosen over
#                         `bundle.externalBin`). Built as a second, separate
#                         `tauri build` invocation, with both a different
#                         `productName` (so the two combos' filenames don't
#                         collide) AND a separate CARGO_TARGET_DIR.
#
#                         The separate target dir isn't optional: Tauri's
#                         AppImage bundler unconditionally does
#                         `fs::remove_dir_all("bundle/appimage")` at the start
#                         of every AppImage build (see tauri-bundler's
#                         src/bundle/linux/appimage/linuxdeploy.rs), so a
#                         second `tauri build` sharing combo 2's target dir
#                         silently deletes combo 2's AppImage the moment it
#                         starts -- a different `productName` alone does not
#                         save it (verified locally: this is exactly what
#                         happened on the first attempt at this script). The
#                         `.deb` bundler has no such wipe, which is why that
#                         half looked fine even with the collision.
#
# Outputs land in their normal Cargo/Tauri locations -- this script does not
# rename or stage anything; see scripts/stage-release.sh for that.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
REPO_ROOT="$(pwd)"

APP_DIR="crates/nova-app"
CLI_BIN="target/release/nova"
DEB_STAGE_DIR="target/release/nova-cli-deb"
CLI_DEB_OUT="target/release/nova-cli.deb"
# Combo 3 gets its own Cargo target dir -- see the comment above for why.
FULL_TARGET_DIR="target/full-bundle"

VERSION="$(sed -n '/^\[workspace\.package\]/,/^\[/p' Cargo.toml \
  | sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' | head -n1)"
if [ -z "$VERSION" ]; then
  echo "build-release-bundles: could not read version from Cargo.toml"
  exit 1
fi
echo "build-release-bundles: building version $VERSION"

echo "== Engine+CLI: cargo build --release -p nova-cli =="
cargo build --release -p nova-cli

if [ ! -f "$CLI_BIN" ]; then
  echo "build-release-bundles: expected CLI binary at $CLI_BIN, not found"
  exit 1
fi

echo "== Engine+CLI: hand-rolling a minimal .deb (dpkg-deb, no cargo-deb) =="
rm -rf "$DEB_STAGE_DIR"
mkdir -p "$DEB_STAGE_DIR/DEBIAN" "$DEB_STAGE_DIR/usr/bin"
cp "$CLI_BIN" "$DEB_STAGE_DIR/usr/bin/nova"
chmod 755 "$DEB_STAGE_DIR/usr/bin/nova"
cat > "$DEB_STAGE_DIR/DEBIAN/control" <<EOF
Package: nova-cli
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Nova Contributors
Description: Nova API client -- command-line interface
 Nova is a local-first API development client whose requests are plain,
 human-readable files that live in a repo and go through normal Git
 workflows. This package installs only the nova CLI (nova-engine +
 nova-cli) -- no desktop GUI.
EOF
dpkg-deb --build --root-owner-group "$DEB_STAGE_DIR" "$CLI_DEB_OUT"
echo "build-release-bundles: wrote $CLI_DEB_OUT"

echo "== Engine+GUI: npm run tauri build (AppImage + deb) =="
(cd "$APP_DIR" && npm run tauri build -- --bundles appimage,deb)

echo "== Engine+CLI+GUI: npm run tauri build with the CLI binary merged in =="
(cd "$APP_DIR" && CARGO_TARGET_DIR="$REPO_ROOT/$FULL_TARGET_DIR" npm run tauri build -- --bundles appimage,deb --config src-tauri/tauri.full-bundle.conf.json)

echo "build-release-bundles: all three combos built"
