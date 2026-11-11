#!/bin/sh
# FigLock installer: curl -fsSL https://figlock.com/install.sh | sh
#
# Downloads the figlock CLI for this machine from the latest GitHub release, checks its SHA-256 against the
# release's SHA256SUMS and installs it. Nothing runs as root unless you point FIGLOCK_INSTALL_DIR at a
# root-owned directory.
#
# Environment:
#   FIGLOCK_VERSION      release tag to install, for example v0.2.0 (default: latest)
#   FIGLOCK_INSTALL_DIR  where to put the binary (default: ~/.local/bin)
#   FIGLOCK_BASE_URL     directory holding the archives and SHA256SUMS (default: the GitHub release); for mirrors and tests
set -eu

repo="figlock/figlock"
install_dir="${FIGLOCK_INSTALL_DIR:-$HOME/.local/bin}"
version="${FIGLOCK_VERSION:-latest}"

say() { printf '%s\n' "$*"; }
fail() { printf 'figlock install: %s\n' "$*" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

case "$(uname -s)" in
  Darwin) target="universal-apple-darwin" ;;
  Linux)
    case "$(uname -m)" in
      x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
      aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
      *) fail "unsupported CPU: $(uname -m)" ;;
    esac
    ;;
  *) fail "unsupported OS: $(uname -s) (macOS and Linux only)" ;;
esac

if [ -n "${FIGLOCK_BASE_URL:-}" ]; then
  base="$FIGLOCK_BASE_URL"
elif [ "$version" = "latest" ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/$version"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading checksums"
curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" || fail "could not download SHA256SUMS from $base"

# The archive name carries the version, so read it back from the checksum list.
archive="$(awk -v t="$target" '$2 ~ ("-" t "\\.tar\\.gz$") { print $2; exit }' "$tmp/SHA256SUMS")"
[ -n "$archive" ] || fail "no build for $target in this release"

say "Downloading $archive"
curl -fsSL "$base/$archive" -o "$tmp/$archive" || fail "could not download $archive"

expected="$(awk -v a="$archive" '$2 == a { print $1; exit }' "$tmp/SHA256SUMS")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$archive" | awk '{ print $1 }')"
else
  actual="$(shasum -a 256 "$tmp/$archive" | awk '{ print $1 }')"
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive, refusing to install"

tar -xzf "$tmp/$archive" -C "$tmp" figlock
mkdir -p "$install_dir"
mv "$tmp/figlock" "$install_dir/figlock"
chmod 755 "$install_dir/figlock"

say "Installed $("$install_dir/figlock" --version) to $install_dir/figlock"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) say "Add $install_dir to your PATH to run figlock from anywhere." ;;
esac
