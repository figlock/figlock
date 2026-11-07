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

zip="$bin.zip"
rm -f "$zip"
ditto -c -k --keepParent "$bin" "$zip"

echo "Submitting to Apple notarization"
notarytool_args=(--key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER")
result=$(xcrun notarytool submit "$zip" "${notarytool_args[@]}" --wait --output-format json)
status=$(printf '%s' "$result" | /usr/bin/plutil -extract status raw -o - -)
id=$(printf '%s' "$result" | /usr/bin/plutil -extract id raw -o - -)
if [ "$status" != "Accepted" ]; then
  echo "Notarization $id finished with status: $status" >&2
  xcrun notarytool log "$id" "${notarytool_args[@]}" >&2 || true
  exit 1
fi

echo "Notarized $id, signed archive at $zip"
