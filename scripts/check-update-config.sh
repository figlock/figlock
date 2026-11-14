#!/usr/bin/env bash
# Fail a release build whose app would ship without a working update key and feed (FIG-112). A build with no
# `plugins.updater` entry silently cannot update, and one with a broken key cannot start, so a release must have
# a well-formed one.
#
# Usage: scripts/check-update-config.sh [tauri.bundle.conf.json]
set -euo pipefail

conf=${1:-"$(dirname "$0")/../apps/desktop/src-tauri/tauri.bundle.conf.json"}
python3 - "$conf" <<'PY'
import base64, json, sys

conf = json.load(open(sys.argv[1]))
u = conf.get("plugins", {}).get("updater")
def fail(msg):
    print(f"update config: {msg}", file=sys.stderr)
    sys.exit(1)

if not u:
    fail("plugins.updater is missing, so this build could never update")
endpoints = u.get("endpoints") or []
if not endpoints or not all(e.startswith("https://") for e in endpoints):
    fail("endpoints must be https URLs")
if u.get("dangerousInsecureTransportProtocol"):
    fail("dangerousInsecureTransportProtocol must not be set in a release")
if u.get("requireSignedVersion") is not True:
    fail("requireSignedVersion must be true, so a manifest cannot pair a new version with an old signed archive")
try:
    text = base64.b64decode(u.get("pubkey", ""), validate=True).decode()
except Exception:
    fail("pubkey is not the base64 text of a minisign public key")
if not text.startswith("untrusted comment:") or len(text.splitlines()) < 2:
    fail("pubkey does not look like a minisign public key")
print("update config ok")
PY
