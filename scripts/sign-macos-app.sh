#!/usr/bin/env bash
# Sign the desktop app and the figlock CLI inside it with the Developer ID Application certificate (hardened
# runtime), notarize and staple the app, then put it in a dmg that is signed, notarized and stapled too
# (FIG-111). Writes FigLock-<version>-<arch>.dmg into <out-dir>.
#
# Usage: scripts/sign-macos-app.sh <FigLock.app> <out-dir>
#
# Environment (as sign-macos.sh): SIGN_IDENTITY, NOTARY_KEY_PATH, NOTARY_KEY_ID, NOTARY_ISSUER, and optionally
# SIGN_KEYCHAIN.
#
# Order matters: nested code is signed before the bundle that contains it, and the app is stapled before it goes
# in the dmg, so it still opens offline once dragged out of the dmg. Neither the app nor the CLI needs
# entitlements (hardened runtime, the login keychain, Touch ID and the Secure Enclave work without any).
set -euo pipefail

app=${1:?usage: sign-macos-app.sh <FigLock.app> <out-dir>}
out=${2:?usage: sign-macos-app.sh <FigLock.app> <out-dir>}
: "${SIGN_IDENTITY:?}" "${NOTARY_KEY_PATH:?}" "${NOTARY_KEY_ID:?}" "${NOTARY_ISSUER:?}"
here=$(dirname "$0")

keychain_args=()
if [ -n "${SIGN_KEYCHAIN:-}" ]; then
  keychain_args=(--keychain "$SIGN_KEYCHAIN")
fi
sign() {
  codesign --force --timestamp ${keychain_args[@]+"${keychain_args[@]}"} --sign "$SIGN_IDENTITY" "$@"
}
need_runtime() {
  # Capture first: `grep -q` closing the pipe early would fail a `pipefail` pipeline.
  local display
  display=$(codesign --display --verbose=2 "$1" 2>&1)
  if ! grep -q 'flags=.*runtime' <<<"$display"; then
    echo "hardened runtime is not enabled on $1" >&2
    exit 1
  fi
}

plist="$app/Contents/Info.plist"
exe_name=$(/usr/bin/plutil -extract CFBundleExecutable raw -o - "$plist")
version=$(/usr/bin/plutil -extract CFBundleShortVersionString raw -o - "$plist")
exe="$app/Contents/MacOS/$exe_name"
cli="$app/Contents/MacOS/figlock"
[ -x "$cli" ] || { echo "no figlock inside $app; build it with scripts/build-desktop.sh" >&2; exit 1; }

archs=$(lipo -archs "$exe")
case "$archs" in
  *" "*) arch=universal ;;
  *) arch=$archs ;;
esac
dmg="$out/FigLock-$version-$arch.dmg"
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "Signing the code inside $app"
# Every executable and library other than the main one first; the bundle itself last.
for f in "$app"/Contents/MacOS/*; do
  [ "$f" = "$exe" ] || sign --options runtime "$f"
done
if [ -d "$app/Contents/Frameworks" ]; then
  while IFS= read -r f; do sign --options runtime "$f"; done \
    < <(find "$app/Contents/Frameworks" \( -name '*.dylib' -o -name '*.framework' \) -prune)
fi
sign --options runtime "$app"
codesign --verify --deep --strict --verbose=2 "$app"
need_runtime "$exe"
need_runtime "$cli"

echo "Notarizing the app"
ditto -c -k --keepParent --norsrc --noextattr --noqtn "$app" "$work/app.zip"
"$here/notarize-macos.sh" "$work/app.zip"
xcrun stapler staple "$app"
xcrun stapler validate "$app"

echo "Building $dmg"
stage="$work/dmg"
mkdir -p "$stage"
ditto "$app" "$stage/$(basename "$app")"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -volname FigLock -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg"
sign "$dmg"
codesign --verify --strict --verbose=2 "$dmg"

echo "Notarizing the dmg"
"$here/notarize-macos.sh" "$dmg"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"

echo "Signed, notarized dmg at $dmg"
