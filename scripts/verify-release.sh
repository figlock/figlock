#!/usr/bin/env bash
# Verify the assets of a staged (draft) release before it is published (FIG-115).
#
# Usage: scripts/verify-release.sh <asset-dir> <tag>
#
# <asset-dir> holds every asset of the release, as it was uploaded. The release is only made public when every
# check passes, because latest.json on the latest release is what every installed app reads: a broken release
# would break updating for everyone.
#
# Environment: FIGLOCK_UPDATE_CONF is the bundle config that holds the public update key (default
# apps/desktop/src-tauri/tauri.bundle.conf.json), and FIGLOCK_UPDATE_BASE_URL is where the update archive must be
# downloaded from (default https://figlock.com/download). Needs `minisign`, `python3`, `tar` and a SHA-256 tool.
set -euo pipefail

dir=${1:?usage: verify-release.sh <asset-dir> <tag>}
tag=${2:?usage: verify-release.sh <asset-dir> <tag>}
here=$(cd "$(dirname "$0")/.." && pwd)
conf=${FIGLOCK_UPDATE_CONF:-"$here/apps/desktop/src-tauri/tauri.bundle.conf.json"}
base=${FIGLOCK_UPDATE_BASE_URL:-https://figlock.com/download}

fail() {
  echo "verify-release: $*" >&2
  exit 1
}
ok() { echo "ok: $*"; }

[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || fail "tag '$tag' is not v<major>.<minor>.<patch>[-suffix]"
version=${tag#v}
[ -d "$dir" ] || fail "$dir is not a directory"
command -v minisign > /dev/null || fail "minisign is not installed, so the update signature cannot be checked"

# 1. Every asset a user or an installed app can ask for is there.
cli=(
  "figlock-$version-universal-apple-darwin.tar.gz"
  "figlock-$version-x86_64-unknown-linux-musl.tar.gz"
  "figlock-$version-aarch64-unknown-linux-musl.tar.gz"
)
dmg="FigLock-$version-universal.dmg"
archive="FigLock-$version-universal.app.tar.gz"
for f in "${cli[@]}" "$dmg" "$archive" "$archive.sig" latest.json SHA256SUMS figlock.rb; do
  [ -s "$dir/$f" ] || fail "asset $f is missing or empty"
done
ok "all assets are present"

# 2. SHA256SUMS lists the archives and the dmg, matches the files, and install.sh finds one archive per target.
sums="$dir/SHA256SUMS"
for f in "${cli[@]}" "$dmg"; do
  grep -qE "^[0-9a-f]{64} [ *]?$f\$" "$sums" || fail "SHA256SUMS does not list $f"
done
if command -v sha256sum > /dev/null; then
  (cd "$dir" && sha256sum --check --quiet SHA256SUMS) || fail "a checksum in SHA256SUMS does not match its file"
else
  (cd "$dir" && shasum -a 256 -c --quiet SHA256SUMS) || fail "a checksum in SHA256SUMS does not match its file"
fi
for t in universal-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
  n=$(awk -v t="$t" '$2 ~ ("-" t "\\.tar\\.gz$")' "$sums" | wc -l | tr -d ' ')
  [ "$n" = 1 ] || fail "SHA256SUMS has $n archives for $t, install.sh needs exactly one"
done
ok "SHA256SUMS matches the files and install.sh finds one archive per target"

# 3. The update manifest names this version, both Macs, and downloads through figlock.com.
python3 - "$dir/latest.json" "$version" "$tag" "$base" "$archive" "$dir/$archive.sig" << 'PY' || exit 1
import json, sys
path, version, tag, base, archive, sigfile = sys.argv[1:7]
def fail(msg):
    print(f"verify-release: latest.json: {msg}", file=sys.stderr)
    sys.exit(1)
try:
    m = json.load(open(path))
except Exception as e:
    fail(f"is not valid JSON ({e})")
if m.get("version") != version:
    fail(f"version is {m.get('version')!r}, the release is {version!r}")
if not str(m.get("notes", "")).strip():
    fail("notes are empty")
platforms = m.get("platforms") or {}
for p in ("darwin-aarch64", "darwin-x86_64"):
    if p not in platforms:
        fail(f"platform {p} is missing")
sig = open(sigfile).read().strip()
for p, e in platforms.items():
    want = f"{base}/{tag}/{archive}"
    if e.get("url") != want:
        fail(f"{p} url is {e.get('url')!r}, expected {want!r}")
    if e.get("signature", "").strip() != sig:
        fail(f"{p} signature is not the one in {archive}.sig")
PY
ok "latest.json names $version, both Macs, and the figlock.com download URL"

# 4. The signature is ours: it verifies against the public key the shipped apps trust, and it is bound to this
# file and this version (the app sets requireSignedVersion, so it refuses an archive signed for another version).
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
python3 - "$conf" "$dir/$archive.sig" "$tmp" << 'PY' || fail "could not read the public key or the signature"
import base64, json, sys
conf, sigfile, tmp = sys.argv[1:4]
pub = json.load(open(conf))["plugins"]["updater"]["pubkey"]
open(f"{tmp}/key.pub", "wb").write(base64.b64decode(pub, validate=True))
open(f"{tmp}/sig.minisig", "wb").write(base64.b64decode(open(sigfile).read().strip(), validate=True))
PY
minisign -V -q -p "$tmp/key.pub" -x "$tmp/sig.minisig" -m "$dir/$archive" > /dev/null 2>&1 \
  || fail "the signature of $archive does not verify against the public key in $(basename "$conf")"
trusted=$(sed -n 's/^trusted comment: //p' "$tmp/sig.minisig")
grep -qF "file:$archive" <<< "$trusted" || fail "the signature is bound to a different file ($trusted)"
grep -qE "version:$version(\$|[[:space:]])" <<< "$trusted" || fail "the signature is bound to a different version ($trusted)"
ok "the update signature verifies and is bound to $archive at $version"

# 5. The archive unpacks cleanly: macOS metadata entries (._*) break the updater, and so does anything outside the app.
entries=$(tar -tzf "$dir/$archive") || fail "$archive is not a readable archive"
if grep -qE '(^|/)\._' <<< "$entries"; then
  fail "$archive has macOS metadata (._) entries"
fi
if grep -qvE '^FigLock\.app(/|$)' <<< "$entries"; then
  fail "$archive has entries outside FigLock.app"
fi
grep -qE '^FigLock\.app/Contents/Info\.plist$' <<< "$entries" || fail "$archive has no FigLock.app/Contents/Info.plist"
ok "the update archive unpacks cleanly"

# 6. The Homebrew formula was filled in for this version.
grep -qF "$version" "$dir/figlock.rb" || fail "figlock.rb does not mention $version"
if grep -q '@[A-Z_0-9]*@' "$dir/figlock.rb"; then
  fail "figlock.rb has an unfilled placeholder"
fi
ok "figlock.rb is filled in for $version"

echo "release $tag verified"
