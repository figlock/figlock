#!/usr/bin/env bash
# Sign a macOS binary with the Developer ID Application certificate (hardened runtime), submit it to Apple
# for notarization and wait for the verdict. Writes a zip of the signed binary next to it.
#
# Usage: scripts/sign-macos.sh <binary>
#
# Environment:
#   SIGN_IDENTITY    certificate name or SHA-1 in the keychain to sign with (required)
#   NOTARY_KEY_PATH  path to the App Store Connect API key (.p8) (required)
#   NOTARY_KEY_ID    API key id (required)
#   NOTARY_ISSUER    API issuer id (required)
#   SIGN_KEYCHAIN    keychain holding the certificate (optional; default keychain search list otherwise)
#
# A bare command-line binary cannot be stapled, so Gatekeeper checks the notarization ticket online the first
# time a quarantined download runs. The signed zip is what gets distributed.
set -euo pipefail

bin=${1:?usage: sign-macos.sh <binary>}
: "${SIGN_IDENTITY:?}" "${NOTARY_KEY_PATH:?}" "${NOTARY_KEY_ID:?}" "${NOTARY_ISSUER:?}"

keychain_args=()
if [ -n "${SIGN_KEYCHAIN:-}" ]; then
  keychain_args=(--keychain "$SIGN_KEYCHAIN")
fi

echo "Signing $bin"
codesign --force --timestamp --options runtime ${keychain_args[@]+"${keychain_args[@]}"} --sign "$SIGN_IDENTITY" "$bin"
codesign --verify --strict --verbose=2 "$bin"
display=$(codesign --display --verbose=2 "$bin" 2>&1)
if ! grep -q 'flags=.*runtime' <<<"$display"; then
  echo "hardened runtime is not enabled on $bin" >&2
  exit 1
fi

# The archive holds the binary at its top level, with no parent directory and no AppleDouble (._) entries, so
# `unzip` gives a single `figlock` that release.yml and the verify step both expect.
zip="$bin.zip"
rm -f "$zip"
(cd "$(dirname "$bin")" && ditto -c -k --norsrc --noextattr --noqtn "$(basename "$bin")" "$(basename "$zip")")

"$(dirname "$0")/notarize-macos.sh" "$zip"
echo "Signed archive at $zip"
