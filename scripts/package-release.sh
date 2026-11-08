#!/usr/bin/env bash
# Turn the per-target binaries into release archives, SHA256SUMS and the Homebrew formula.
#
# Usage: scripts/package-release.sh <version> <input-dir> <output-dir>
#
# <input-dir> holds one directory per target, each with a `figlock` binary:
#   universal-apple-darwin/  x86_64-unknown-linux-musl/  aarch64-unknown-linux-musl/
# <output-dir> gets figlock-<version>-<target>.tar.gz for each, SHA256SUMS and figlock.rb.
set -euo pipefail

version=${1:?usage: package-release.sh <version> <input-dir> <output-dir>}
in=${2:?}
out=${3:?}
targets=(universal-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl)

mkdir -p "$out"
for t in "${targets[@]}"; do
  bin="$in/$t/figlock"
  [ -f "$bin" ] || { echo "missing binary for $t at $bin" >&2; exit 1; }
  chmod 755 "$bin"
  tar -czf "$out/figlock-$version-$t.tar.gz" -C "$in/$t" figlock
done

(cd "$out" && shasum -a 256 figlock-*.tar.gz > SHA256SUMS)

sha() { awk -v f="figlock-$version-$1.tar.gz" '$2 == f { print $1 }' "$out/SHA256SUMS"; }
sed -e "s/@VERSION@/$version/g" \
  -e "s/@SHA_MACOS@/$(sha universal-apple-darwin)/" \
  -e "s/@SHA_LINUX_X86_64@/$(sha x86_64-unknown-linux-musl)/" \
  -e "s/@SHA_LINUX_AARCH64@/$(sha aarch64-unknown-linux-musl)/" \
  "$(dirname "$0")/../packaging/homebrew/figlock.rb.in" > "$out/figlock.rb"

if grep -q '@[A-Z_0-9]*@' "$out/figlock.rb"; then
  echo "unfilled placeholder in figlock.rb" >&2
  exit 1
fi
echo "Packaged $version into $out"
