#!/usr/bin/env bash
# Put the release notes into the update manifest, so the app's "What is new" shows what changed (FIG-115).
#
# Usage: scripts/set-update-notes.sh <latest.json> <notes-file>
#
# The notes file is what GitHub generates for the release ("* title by @author in https://.../pull/127"). The app
# shows the manifest's text as it is, so each of those lines becomes "- title (#127)", and the lines that only
# make sense on GitHub (the "What's Changed" heading, the changelog link, new-contributor lines) are dropped.
# Text that is not in GitHub's format is kept as written. The notes are not part of the signature (that covers
# the archive and its version), so they can be set after the update is packaged. Over-long notes are cut, and
# empty notes leave the manifest as it was.
set -euo pipefail

manifest=${1:?usage: set-update-notes.sh <latest.json> <notes-file>}
notes=${2:?usage: set-update-notes.sh <latest.json> <notes-file>}

python3 - "$manifest" "$notes" << 'PY'
import json, re, sys

manifest, notes = sys.argv[1:3]

PULL = re.compile(r"^[*-] (.+) by @\S+ in https?://\S+/pull/(\d+)\s*$")
DROP = (
    re.compile(r"^#+ What'?s Changed\s*$", re.I),
    re.compile(r"^#+ New Contributors\s*$", re.I),
    re.compile(r"^[*-] @\S+ made their first contribution", re.I),
    re.compile(r"^\*\*Full Changelog\*\*", re.I),
)


def plain(text):
    out = []
    for line in text.splitlines():
        line = line.rstrip()
        if any(p.match(line) for p in DROP):
            continue
        m = PULL.match(line)
        out.append(f"- {m.group(1)} (#{m.group(2)})" if m else line)
    # Dropping lines can leave runs of blank ones.
    return re.sub(r"\n{3,}", "\n\n", "\n".join(out)).strip()


text = plain(open(notes).read())
if not text:
    print("no release notes, the manifest keeps its own", file=sys.stderr)
    sys.exit(0)
limit = 2000
if len(text) > limit:
    cut = text[: limit - 1]
    # Cut at a line end so a title is not left half written.
    if "\n" in cut:
        cut = cut[: cut.rindex("\n")]
    text = cut.rstrip() + "…"
m = json.load(open(manifest))
m["notes"] = text
json.dump(m, open(manifest, "w"), indent=2)
PY
