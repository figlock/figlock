#!/usr/bin/env bash
# Build the desktop app with the figlock command line tool inside it (FIG-110).
#
# Usage: scripts/build-desktop.sh [target]
#   target: a Rust target triple, or universal-apple-darwin. Defaults to this machine's.
#
# The tool is built from the same commit and put where Tauri expects a sidecar
# (apps/desktop/src-tauri/binaries/figlock-<target>); `tauri.bundle.conf.json` then bundles it as
# FigLock.app/Contents/MacOS/figlock. That config is only merged here, so ordinary builds, tests and lint
# do not need the binary. Signing and the dmg are separate steps (FIG-111).
set -euo pipefail

cd "$(dirname "$0")/.."
target=${1:-$(rustc -vV | sed -n 's/^host: //p')}
bins=apps/desktop/src-tauri/binaries
mkdir -p "$bins"

build_cli() { cargo build --release --locked -p figlock-cli --target "$1"; }

case "$target" in
  universal-apple-darwin)
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    build_cli aarch64-apple-darwin
    build_cli x86_64-apple-darwin
    # Tauri builds each architecture on its own and then joins them, so it looks for a sidecar per
    # architecture as well as the universal one.
    cp target/aarch64-apple-darwin/release/figlock "$bins/figlock-aarch64-apple-darwin"
    cp target/x86_64-apple-darwin/release/figlock "$bins/figlock-x86_64-apple-darwin"
    lipo -create -output "$bins/figlock-$target" \
      target/aarch64-apple-darwin/release/figlock \
      target/x86_64-apple-darwin/release/figlock
    ;;
  *)
    build_cli "$target"
    cp "target/$target/release/figlock" "$bins/figlock-$target"
    ;;
esac

cd apps/desktop
pnpm tauri build --target "$target" --config src-tauri/tauri.bundle.conf.json
