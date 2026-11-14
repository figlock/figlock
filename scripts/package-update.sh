#!/usr/bin/env bash
# Package the signed, stapled desktop app as the archive the in-app updater downloads, sign it with the update
# key and write the manifest the app reads (FIG-112).
#
# Usage: scripts/package-update.sh <FigLock.app> <out-dir>
#
# Environment: TAURI_SIGNING_PRIVATE_KEY (the key, or TAURI_SIGNING_PRIVATE_KEY_PATH) and
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD. FIGLOCK_UPDATE_ALLOW_SINGLE_ARCH=1 accepts a single-architecture app (local
# tests of the updater only; a release is always universal). FIGLOCK_UPDATE_BASE_URL is where the archive will be downloaded from
# (default https://figlock.com/download), and FIGLOCK_UPDATE_NOTES the text shown as "What is new".
#
# Writes into <out-dir>: FigLock-<version>-universal.app.tar.gz, its .sig and latest.json. The archive is made
# from the app AFTER it was signed and notarized, so the update installs the same signed bundle (the CLI inside
# included), and it carries no macOS metadata entries (`._*`), which the updater cannot unpack.
set -euo pipefail

app=${1:?usage: package-update.sh <FigLock.app> <out-dir>}
out=${2:?usage: package-update.sh <FigLock.app> <out-dir>}
base=${FIGLOCK_UPDATE_BASE_URL:-https://figlock.com/download}

version=$(/usr/bin/plutil -extract CFBundleShortVersionString raw -o - "$app/Contents/Info.plist")
exe=$(/usr/bin/plutil -extract CFBundleExecutable raw -o - "$app/Contents/Info.plist")
archs=$(lipo -archs "$app/Contents/MacOS/$exe")
platforms=darwin-aarch64,darwin-x86_64
case "$archs" in
  *arm64*x86_64* | *x86_64*arm64*) ;;
  *)
    [ "${FIGLOCK_UPDATE_ALLOW_SINGLE_ARCH:-}" = 1 ] || {
      echo "the update archive is for a universal app, this one is: $archs" >&2
      exit 1
    }
    case "$archs" in
      arm64) platforms=darwin-aarch64 ;;
      *) platforms=darwin-x86_64 ;;
    esac
    ;;
esac
codesign --verify --deep --strict "$app"

# Absolute: the signer below runs from another directory, and a relative path would not find the archive.
mkdir -p "$out"
out=$(cd "$out" && pwd)

name="FigLock-$version-universal.app.tar.gz"
rm -f "$out/$name" "$out/$name.sig"
COPYFILE_DISABLE=1 tar -czf "$out/$name" -C "$(dirname "$app")" "$(basename "$app")"

# The updater unpacks every entry under the app's own folder, so a stray entry breaks the install.
entries=$(tar -tzf "$out/$name")
if grep -qE '(^|/)\._' <<<"$entries"; then
  echo "the archive has macOS metadata (._) entries" >&2
  exit 1
fi
if grep -qvE '^FigLock\.app(/|$)' <<<"$entries"; then
  echo "the archive has entries outside FigLock.app" >&2
  exit 1
fi

here=$(cd "$(dirname "$0")/.." && pwd)
(cd "$here/apps/desktop" && pnpm tauri signer sign --app-version "$version" "$out/$name") >&2
[ -s "$out/$name.sig" ] || { echo "signing wrote no signature" >&2; exit 1; }

python3 - "$out" "$name" "$version" "$base" "${FIGLOCK_UPDATE_NOTES:-FigLock $version}" "$platforms" <<'PY'
import json, sys, datetime
out, name, version, base, notes, platforms = sys.argv[1:7]
sig = open(f"{out}/{name}.sig").read().strip()
entry = {"signature": sig, "url": f"{base}/v{version}/{name}"}
manifest = {
    "version": version,
    "notes": notes,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    # One universal archive serves both Mac architectures.
    "platforms": {p: entry for p in platforms.split(",")},
}
json.dump(manifest, open(f"{out}/latest.json", "w"), indent=2)
PY
echo "Update archive, signature and manifest for $version are in $out"
