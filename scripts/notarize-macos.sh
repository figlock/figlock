#!/usr/bin/env bash
# Submit a file to Apple for notarization and wait for the verdict. Prints the notary log and fails unless the
# status is Accepted. `sign-macos.sh` (the CLI) and `sign-macos-app.sh` (the desktop app) both use it.
#
# Usage: scripts/notarize-macos.sh <zip, dmg or pkg>
#
# Environment: NOTARY_KEY_PATH (the App Store Connect API key, .p8), NOTARY_KEY_ID and NOTARY_ISSUER, all required.
set -euo pipefail

file=${1:?usage: notarize-macos.sh <file>}
: "${NOTARY_KEY_PATH:?}" "${NOTARY_KEY_ID:?}" "${NOTARY_ISSUER:?}"

echo "Submitting $file to Apple notarization"
notarytool_args=(--key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER")
result=$(xcrun notarytool submit "$file" "${notarytool_args[@]}" --wait --output-format json) || true
status=$(printf '%s' "$result" | /usr/bin/plutil -extract status raw -o - - 2>/dev/null) || status=unknown
id=$(printf '%s' "$result" | /usr/bin/plutil -extract id raw -o - - 2>/dev/null) || id=
if [ "$status" != "Accepted" ]; then
  echo "Notarization ${id:-without a submission id} finished with status: $status" >&2
  if [ -n "$id" ]; then
    xcrun notarytool log "$id" "${notarytool_args[@]}" >&2 || true
  else
    printf '%s\n' "$result" >&2
  fi
  exit 1
fi
echo "Notarized $id"
