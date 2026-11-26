#!/usr/bin/env bash
# Print the iPhone app's version numbers for a release tag (FIG-175), as KEY=VALUE lines for $GITHUB_ENV.
#
#   MARKETING_VERSION      the version people see, x.y.z. A pre-release suffix is dropped (App Store Connect only
#                          takes numbers), so v0.6.0-rc1 builds as 0.6.0.
#   CURRENT_PROJECT_VERSION the build number. App Store Connect wants it to go up for every upload of the same
#                          version, so the workflow passes its own run number.
#
# The app follows the Cargo workspace version like the Mac app and the CLI, and the tag must be v<that version>.
#
# Usage: scripts/ios-version.sh <tag> <build-number>
#   CARGO_TOML overrides the file the version is read from (the tests use it).
set -euo pipefail

tag=${1:?usage: ios-version.sh <tag> <build-number>}
build=${2:?usage: ios-version.sh <tag> <build-number>}
root=$(cd "$(dirname "$0")/.." && pwd)
cargo_toml=${CARGO_TOML:-$root/Cargo.toml}

version=$(awk -F'"' '/^\[workspace.package\]/ {p=1} p && /^version = / {print $2; exit}' "$cargo_toml")
[ -n "$version" ] || { echo "could not read the workspace version from $cargo_toml" >&2; exit 1; }
if [ "v$version" != "$tag" ]; then
  echo "the tag $tag does not match the workspace version $version" >&2
  exit 1
fi

marketing=${version%%-*}
if ! [[ $marketing =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "the version $version has no x.y.z core that App Store Connect accepts" >&2
  exit 1
fi
if ! [[ $build =~ ^[1-9][0-9]*$ ]]; then
  echo "the build number '$build' is not a positive integer" >&2
  exit 1
fi

echo "MARKETING_VERSION=$marketing"
echo "CURRENT_PROJECT_VERSION=$build"
