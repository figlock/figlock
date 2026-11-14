#!/usr/bin/env bash
# Put the release notes into the update manifest, so the app's "What is new" shows what changed (FIG-115).
#
# Usage: scripts/set-update-notes.sh <latest.json> <notes-file>
#
# The notes are not part of the signature (that covers the archive and its version), so they can be set after the
# update is packaged. Over-long notes are cut, and empty notes leave the manifest as it was.
set -euo pipefail

manifest=${1:?usage: set-update-notes.sh <latest.json> <notes-file>}
notes=${2:?usage: set-update-notes.sh <latest.json> <notes-file>}

python3 - "$manifest" "$notes" << 'PY'
import json, sys
manifest, notes = sys.argv[1:3]
text = open(notes).read().strip()
if not text:
    print("no release notes, the manifest keeps its own", file=sys.stderr)
    sys.exit(0)
limit = 2000
if len(text) > limit:
    text = text[: limit - 1].rstrip() + "…"
m = json.load(open(manifest))
m["notes"] = text
json.dump(m, open(manifest, "w"), indent=2)
PY
