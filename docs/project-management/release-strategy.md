# Release Strategy

## Bundle matrix

Every release ships as three separate downloadable bundles, all cut from one
coordinated version number (see the decision in issue #71):

| Bundle           | Contents                          | Who it's for                          |
| ---------------- | ---------------------------------- | -------------------------------------- |
| Engine+CLI        | `nova-engine` + the `nova` CLI      | scripting/automation/headless/CI use   |
| Engine+GUI        | `nova-engine` + the desktop app     | the typical desktop user                |
| Engine+CLI+GUI    | `nova-engine` + the CLI + the app   | power users who want both on one box    |

`nova-engine` isn't independently installable — it's a library, not a
separate download — but it's what every bundle is actually built from, so it
appears in every row above.

Independent per-crate versioning was considered and rejected in #71: the
project doesn't have a reason yet to ship the CLI and GUI on different
cadences, and one version number is simpler for users to reason about.

## Package formats

First release targets **AppImage** and **`.deb`** only, since they're the
lowest-maintenance, broadest-reach combination for Linux. RPM, Flatpak, and
AUR packaging are staged for later once this pipeline has actually shipped a
release or two — see #71 for the reasoning.

The Engine+CLI bundle ships as a `.tar.gz` binary archive and a `.deb`; it
deliberately skips AppImage, since a headless CLI tool doesn't benefit from a
self-contained desktop package format. That's a reversible scope decision,
not an oversight — revisit it if there's ever a concrete reason to want a
CLI-only AppImage.

## Version-bearing files

One version, kept in sync across three files:

- `Cargo.toml` — `[workspace.package].version`. `nova-engine`, `nova-cli`,
  and `nova-app`'s Tauri backend all inherit it via `version.workspace = true`,
  so this one file covers the whole Rust side.
- `crates/nova-app/package.json` — `version`.
- `crates/nova-app/src-tauri/tauri.conf.json` — `version`.

`scripts/verify-release-version.sh <tag>` checks all three agree with the
semver parsed out of a release tag; CI runs it before any bundle is built, so
a forgotten version bump fails the build instead of shipping a
version-mismatched release.

## Tag format

`v<VERSION>`, optionally with a slugified title: `v0.2.0` or
`v0.2.0-some-title`. Pushing a tag matching `v*` triggers
`.github/workflows/release.yml`.

## Producing a release

1. `make release VER=0.2.0 TITLE="Optional title"` (both prompt if omitted;
   `VER` shows the current version as a hint). This runs
   `scripts/release-checks.sh` (formatting, clippy, tests, frontend
   type-check/build), bumps the three version files above, commits
   `Release v0.2.0`, and creates an annotated tag. It does not push anything
   — it prints `git push origin main --tags` as the next step, left to a
   human.
   - `make release-skip-tests` (or `SKIP_CHECKS=1`) skips the checks step,
     for when they've already been run separately.
2. Pushing the tag triggers `.github/workflows/release.yml`, which:
   - parses the version out of the tag and runs
     `scripts/verify-release-version.sh` against it,
   - installs the same Tauri Linux system dependencies as `.github/workflows/ci.yml`,
   - runs `scripts/build-release-bundles.sh` to build all three combos,
   - runs `scripts/stage-release.sh` to collect and rename the built
     artifacts into `release-files/`,
   - publishes a **draft** GitHub Release (via `softprops/action-gh-release`)
     with the staged files attached and auto-generated release notes.
3. A human reviews the draft release and publishes it manually. Nothing in
   this pipeline auto-publishes.

There is no code-signing, auto-update mechanism, or `latest.json` manifest in
this pipeline — Nova hasn't decided to have an updater, so none of that
machinery exists yet.

## Where the pipeline lives

- `scripts/release-checks.sh` — pre-release validation gate (fmt, clippy,
  tests, frontend build). Also runnable standalone as `make release-checks`.
- `scripts/verify-release-version.sh <tag>` — asserts the three version files
  above agree with a tag's semver.
- `scripts/build-release-bundles.sh` — builds all three bundle combos.
  Engine+CLI and Engine+GUI land in the normal Cargo/Tauri output locations
  (`target/release/`, `target/release/bundle/{appimage,deb}/`).
  Engine+CLI+GUI is built with its own `CARGO_TARGET_DIR`
  (`target/full-bundle/`), landing under
  `target/full-bundle/release/bundle/{appimage,deb}/` — see below for why it
  needs a separate target dir, not just a different output filename. Also
  runnable as `make build-release-bundles`.
- `scripts/stage-release.sh <version> [out_dir]` — copies/renames the built
  artifacts from both target directories into `release-files/` (default)
  with predictable names: `nova-cli-v<ver>-linux-x86_64.tar.gz`,
  `nova-cli_<ver>_amd64.deb`, `nova-gui_<ver>_amd64.AppImage`,
  `nova-gui_<ver>_amd64.deb`, `nova-full_<ver>_amd64.AppImage`,
  `nova-full_<ver>_amd64.deb`. Fails loudly if an expected artifact is
  missing. Also runnable as `make stage-release VER=<ver>`.
- `crates/nova-app/src-tauri/tauri.full-bundle.conf.json` — a small Tauri
  config merged in for the Engine+CLI+GUI build. It maps the already-built
  `nova` CLI binary to `usr/bin/nova` inside both the `.deb` and the AppImage
  via `bundle.linux.{deb,appimage}.files` (the map is
  `{destination_in_package: source_path_on_disk}` — confirmed against
  `tauri-bundler`'s own `copy_custom_files`, since the public docs describe
  the two directions inconsistently), and overrides `productName` to
  `nova-full` so this build's output filenames don't collide with the plain
  Engine+GUI build's.

  `productName` alone isn't enough to keep the two builds' AppImages apart,
  though: `tauri-bundler`'s AppImage step unconditionally deletes the whole
  `bundle/appimage/` directory at the start of every AppImage build,
  regardless of `productName` (its `.deb` step has no such wipe, which is
  why that half looked fine on its own). Building Engine+CLI+GUI with a
  separate `CARGO_TARGET_DIR` gives it a fully separate output tree so it
  can't clobber Engine+GUI's AppImage — confirmed locally by first hitting
  exactly that collision (Engine+GUI's AppImage disappeared after the second
  build ran) and then rebuilding both from clean with the separate target
  dir and finding both AppImages present afterward.

  For the `.deb`, `usr/bin/nova` lands on `PATH` after a normal install, the
  same as any other package-managed binary. AppImages are self-contained,
  read-only images with no install step, so a binary at `usr/bin/nova`
  inside one is not automatically reachable from the host's `PATH` the way
  it is for a `.deb` — confirmed locally: extracting the built AppImage
  (`./nova-full_<ver>_amd64.AppImage --appimage-extract`) shows
  `squashfs-root/usr/bin/nova` present and executable, but nothing about a
  normal AppImage run puts it on `PATH` the way installing a `.deb` does.
  Using the CLI from an AppImage install currently means extracting the
  image and invoking the binary directly, or running it via
  `--appimage-extract-and-run`-style tooling. That's noted here rather than
  solved, since it doesn't block the `.deb`-based path and Nova hasn't
  decided on an AppImage-specific fix.
- `.github/workflows/release.yml` — the tag-triggered CI job that runs the
  scripts above and publishes the draft release.

`nova-cli`'s `.deb` is a minimal hand-rolled package (a `DEBIAN/control` file
plus the binary at `usr/bin/nova`, built with `dpkg-deb --build`) rather than
going through `cargo-deb` or another packaging crate — matching the
lowest-maintenance spirit of the format choice above, and keeping the release
pipeline's dependencies to what's already installed on the CI runner.
