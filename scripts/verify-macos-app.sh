#!/usr/bin/env bash
# Check a release dmg of the desktop app the way Gatekeeper and a person would (FIG-111): the dmg and the app and
# the CLI inside it are signed with a Developer ID, have the hardened runtime and carry a stapled notarization
# ticket, and the CLI runs from inside the app.
#
# Usage: scripts/verify-macos-app.sh <FigLock-*.dmg>
set -euo pipefail

dmg=${1:?usage: verify-macos-app.sh <dmg>}
mnt=$(mktemp -d)
cleanup() {
  hdiutil detach "$mnt" -quiet || hdiutil detach "$mnt" -force -quiet || true
  rmdir "$mnt" 2>/dev/null || true
}
trap cleanup EXIT

fail() { echo "verify: $*" >&2; exit 1; }

signed_properly() {
  local info
  info=$(codesign --display --verbose=4 "$1" 2>&1)
  grep -q 'Authority=Developer ID Application' <<<"$info" || fail "$1 is not signed with a Developer ID Application certificate"
  grep -q 'Timestamp=' <<<"$info" || fail "$1 has no secure timestamp"
}
hardened() {
  local info
  info=$(codesign --display --verbose=2 "$1" 2>&1)
  grep -q 'flags=.*runtime' <<<"$info" || fail "hardened runtime is off on $1"
}

echo "The dmg"
codesign --verify --strict --verbose=2 "$dmg"
signed_properly "$dmg"
xcrun stapler validate "$dmg"
spctl --assess --type open --context context:primary-signature -vv "$dmg"

hdiutil attach "$dmg" -nobrowse -readonly -mountpoint "$mnt" -quiet
app="$mnt/FigLock.app"
[ -d "$app" ] || fail "no FigLock.app in the dmg"
[ -L "$mnt/Applications" ] || fail "no Applications shortcut in the dmg"

echo "The app"
codesign --verify --deep --strict --verbose=2 "$app"
exe="$app/Contents/MacOS/$(/usr/bin/plutil -extract CFBundleExecutable raw -o - "$app/Contents/Info.plist")"
cli="$app/Contents/MacOS/figlock"
for f in "$app" "$exe" "$cli"; do
  signed_properly "$f"
  hardened "$f"
done
xcrun stapler validate "$app"
spctl --assess --type execute -vv "$app"

echo "The command line tool inside"
[ "$(lipo -archs "$exe")" = "$(lipo -archs "$cli")" ] || fail "the app and the CLI are built for different architectures"
"$cli" --version

echo "Verified $dmg"
