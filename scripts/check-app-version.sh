#!/usr/bin/env bash
# Fail when the built desktop app does not carry the release version (FIG-112). The app takes its version from
# the Cargo workspace (`apps/desktop/src-tauri/tauri.conf.json` has none of its own), and the updater compares
# that number with the manifest's, so a stale one would make installed apps think they are already up to date or
# offer the wrong version.
#
# Usage: scripts/check-app-version.sh <FigLock.app> [expected-version]
#   expected-version defaults to the workspace version in Cargo.toml. On a tag build (GITHUB_REF_TYPE=tag) the
#   tag must be v<that version> as well.
set -euo pipefail

app=${1:?usage: check-app-version.sh <FigLock.app> [expected-version]}
root=$(cd "$(dirname "$0")/.." && pwd)
workspace=$(awk -F'"' '/^\[workspace.package\]/ {p=1} p && /^version = / {print $2; exit}' "$root/Cargo.toml")
expected=${2:-$workspace}
[ -n "$expected" ] || { echo "could not read the workspace version from Cargo.toml" >&2; exit 1; }

built=$(/usr/bin/plutil -extract CFBundleShortVersionString raw -o - "$app/Contents/Info.plist")
if [ "$built" != "$expected" ]; then
  echo "the app is version $built but the release is $expected" >&2
  exit 1
fi
if [ "${GITHUB_REF_TYPE:-}" = tag ] && [ "${GITHUB_REF_NAME:-}" != "v$expected" ]; then
  echo "the tag ${GITHUB_REF_NAME:-} does not match the version $expected" >&2
  exit 1
fi
echo "the app is version $built"
