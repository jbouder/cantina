#!/usr/bin/env bash
# Builds the rustdoc for the library crates and copies it into public/api,
# where the site serves it at /api/. macOS only, like the crates.
#
#   scripts/rustdoc.sh
set -euo pipefail

docs="$(cd "$(dirname "$0")/.." && pwd)"
cd "$docs/.."
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps \
  -p cantina-dsp -p cantina-engine -p cantina-hosting -p cantina-project -p cantina-cli
rm -rf "$docs/public/api"
cp -R target/doc "$docs/public/api"
