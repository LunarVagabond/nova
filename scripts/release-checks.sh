#!/usr/bin/env bash
# Pre-release validation gate: formatting, clippy, Rust tests, and the
# frontend type-check/build.
#
# Run before `make release` bumps version files/commits/tags, so a broken
# build fails fast locally instead of surfacing after a tag is already
# pushed to CI (which means: delete tag, delete draft release, fix, re-tag).
#
# Kept as a standalone script (rather than inlined in the Makefile release
# recipe) so CI can call the same gate without duplicating the logic.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

echo "release-checks: cargo fmt --check (make fmt-check)"
make fmt-check

echo "release-checks: clippy across the workspace (make lint)"
make lint

echo "release-checks: cargo test (make test)"
make test

echo "release-checks: frontend type-check + build (npm run build)"
(cd crates/nova-app && npm run build)

echo "release-checks: all checks passed"
