#!/usr/bin/env bash
# Tests for scripts/verify-release.sh and scripts/set-update-notes.sh (FIG-115): a good release passes, and
# each thing the verification checks is broken in turn and must make it fail for that reason.
#
# Needs `minisign`. Uses a throw-away key, never the real update key.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
verify="$here/verify-release.sh"
work=$(mktemp -d)
[ -n "$work" ] && [ -d "$work" ] || exit 2
trap 'rm -rf -- "${work:?}"' EXIT

command -v minisign > /dev/null || {
  echo "minisign is required" >&2
  exit 2
}

version=9.9.9
tag=v$version
base=https://figlock.com/download
archive="FigLock-$version-universal.app.tar.gz"
r="$work/r" # the release under test

b64() { python3 -c 'import base64,sys;sys.stdout.write(base64.b64encode(sys.stdin.buffer.read()).decode())'; }

# A throw-away key, and a config holding its public half the way tauri.bundle.conf.json does.
minisign -G -W -f -p "$work/key.pub" -s "$work/key.sec" > /dev/null
minisign -G -W -f -p "$work/other.pub" -s "$work/other.sec" > /dev/null
conf_for() { # <pub-file> <conf-file>
  python3 - "$1" "$2" << 'PY'
import base64, json, sys
pub, out = sys.argv[1:3]
json.dump({"plugins": {"updater": {"pubkey": base64.b64encode(open(pub, "rb").read()).decode()}}}, open(out, "w"))
PY
}
conf_for "$work/key.pub" "$work/conf.json"
conf_for "$work/other.pub" "$work/conf-other.json"

# Sign a file the way `tauri signer sign --app-version` does: the .sig is the base64 of a minisign signature whose
# trusted comment names the file and the version.
sign() { # <file> <version> <secret-key>
  minisign -S -q -s "$3" -m "$1" -t "timestamp:1	file:$(basename "$1")	version:$2" -x "$1.minisig"
  b64 < "$1.minisig" > "$1.sig"
  rm -f -- "$1.minisig"
}

# Point every platform of the manifest at the signature file that is next to it.
resync_manifest() {
  python3 - "$r" "$archive" << 'PY'
import json, sys
d, a = sys.argv[1:3]
m = json.load(open(f"{d}/latest.json")); sig = open(f"{d}/{a}.sig").read().strip()
for e in m["platforms"].values(): e["signature"] = sig
json.dump(m, open(f"{d}/latest.json", "w"))
PY
}

# Build the app folder the update archive is made from, with or without a macOS metadata entry.
make_app() { # <dir> [with-metadata]
  mkdir -p "$1/FigLock.app/Contents/MacOS"
  echo '<plist/>' > "$1/FigLock.app/Contents/Info.plist"
  echo bin > "$1/FigLock.app/Contents/MacOS/FigLock"
  if [ "${2:-}" = with-metadata ]; then echo x > "$1/FigLock.app/._FigLock"; fi
}

# A good release in $r.
make_release() {
  rm -rf -- "${r:?}"
  mkdir -p "$r"
  rm -rf -- "${work:?}/app"
  make_app "$work/app"
  COPYFILE_DISABLE=1 tar -czf "$r/$archive" -C "$work/app" FigLock.app
  sign "$r/$archive" "$version" "$work/key.sec"
  for t in universal-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
    echo "figlock $t" > "$work/figlock"
    tar -czf "$r/figlock-$version-$t.tar.gz" -C "$work" figlock
  done
  echo dmg > "$r/FigLock-$version-universal.dmg"
  write_sums
  echo "class Figlock < Formula; version \"$version\"; end" > "$r/figlock.rb"
  python3 - "$r" "$version" "$base" "$archive" << 'PY'
import json, sys
d, version, base, archive = sys.argv[1:5]
sig = open(f"{d}/{archive}.sig").read().strip()
entry = {"signature": sig, "url": f"{base}/v{version}/{archive}"}
json.dump({"version": version, "notes": "FigLock v" + version, "pub_date": "2026-01-01T00:00:00Z",
           "platforms": {"darwin-aarch64": entry, "darwin-x86_64": entry}}, open(f"{d}/latest.json", "w"))
PY
}
write_sums() { (cd "$r" && shasum -a 256 figlock-*.tar.gz FigLock-*-universal.dmg > SHA256SUMS); }
drop() { rm -f -- "${r:?}/${1:?}"; } # remove one asset of the release under test

passed=0
failed=0
pass() {
  echo "PASS  $1"
  passed=$((passed + 1))
}
bad() {
  echo "FAIL  $1"
  failed=$((failed + 1))
}

expect_ok() { # <name> <dir> [tag] [conf]
  if FIGLOCK_UPDATE_CONF=${4:-$work/conf.json} "$verify" "$2" "${3:-$tag}" > "$work/out" 2>&1; then pass "$1"; else
    bad "$1"
    cat "$work/out"
  fi
}
expect_fail() { # <name> <expected message> <dir> [tag] [conf]
  if FIGLOCK_UPDATE_CONF=${5:-$work/conf.json} "$verify" "$3" "${4:-$tag}" > "$work/out" 2>&1; then
    bad "$1 (verification passed)"
  elif grep -qF -- "$2" "$work/out"; then pass "$1"; else
    bad "$1 (failed for another reason)"
    cat "$work/out"
  fi
}

make_release
expect_ok "a good release passes" "$r"
expect_fail "a tag that is not a version" "is not v<major>" "$r" release-1
expect_fail "the asset directory does not exist" "is not a directory" "$work/nowhere"

make_release
drop "FigLock-$version-universal.dmg"
expect_fail "the dmg is missing" "asset FigLock-$version-universal.dmg is missing" "$r"

make_release
drop latest.json
expect_fail "latest.json is missing" "asset latest.json is missing" "$r"

make_release
drop "$archive.sig"
expect_fail "the signature file is missing" "$archive.sig is missing" "$r"

make_release
echo tampered >> "$r/figlock-$version-x86_64-unknown-linux-musl.tar.gz"
expect_fail "a CLI archive changed after SHA256SUMS was written" "a checksum in SHA256SUMS does not match" "$r"

make_release
grep -v 'aarch64-unknown-linux-musl' "$r/SHA256SUMS" > "$r/s" && mv "$r/s" "$r/SHA256SUMS"
expect_fail "SHA256SUMS lacks an archive" "SHA256SUMS does not list figlock-$version-aarch64-unknown-linux-musl.tar.gz" "$r"

make_release
python3 - "$r/latest.json" << 'PY'
import json, sys
m = json.load(open(sys.argv[1])); m["version"] = "9.9.8"; json.dump(m, open(sys.argv[1], "w"))
PY
expect_fail "the manifest names another version than the tag" "version is '9.9.8', the release is '$version'" "$r"

make_release
python3 - "$r/latest.json" << 'PY'
import json, sys
m = json.load(open(sys.argv[1])); del m["platforms"]["darwin-x86_64"]; json.dump(m, open(sys.argv[1], "w"))
PY
expect_fail "the manifest lacks an Intel Mac" "platform darwin-x86_64 is missing" "$r"

make_release
sed -i.bak 's#https://figlock.com/download#https://example.com/download#g' "$r/latest.json"
drop latest.json.bak
expect_fail "the manifest downloads from another host" "url is 'https://example.com/download" "$r"

make_release
python3 - "$r/latest.json" << 'PY'
import json, sys
m = json.load(open(sys.argv[1])); m["notes"] = "  "; json.dump(m, open(sys.argv[1], "w"))
PY
expect_fail "the manifest has no notes" "notes are empty" "$r"

make_release
echo '{' > "$r/latest.json"
expect_fail "the manifest is not JSON" "is not valid JSON" "$r"

make_release
echo tampered >> "$r/$archive"
write_sums
expect_fail "the update archive was changed after it was signed" "does not verify against the public key" "$r"

make_release
expect_fail "signed with a key the apps do not trust" "does not verify against the public key" "$r" "$tag" "$work/conf-other.json"

make_release
sign "$r/$archive" 9.9.8 "$work/key.sec"
resync_manifest
expect_fail "signed for another version" "the signature is bound to a different version" "$r"

make_release
# Built with tarfile: macOS tar drops ._ files, which is exactly what must not be left in a release.
python3 - "$r/$archive" << 'PY'
import io, sys, tarfile
with tarfile.open(sys.argv[1], "w:gz") as t:
    for name, data in (("FigLock.app/Contents/Info.plist", b"<plist/>"), ("FigLock.app/._FigLock", b"x")):
        info = tarfile.TarInfo(name); info.size = len(data); t.addfile(info, io.BytesIO(data))
PY
sign "$r/$archive" "$version" "$work/key.sec"
resync_manifest
expect_fail "the update archive has a macOS metadata entry" "has macOS metadata" "$r"

make_release
echo 'class Figlock < Formula; version "@VERSION@"; end' > "$r/figlock.rb"
expect_fail "the Homebrew formula was not filled in" "figlock.rb does not mention $version" "$r"

make_release
echo "class Figlock < Formula; version \"$version\"; sha256 \"@SHA_MACOS@\"; end" > "$r/figlock.rb"
expect_fail "the Homebrew formula has a placeholder" "figlock.rb has an unfilled placeholder" "$r"

# set-update-notes.sh
make_release
printf '## What is new\n- a fix\n' > "$work/notes.md"
"$here/set-update-notes.sh" "$r/latest.json" "$work/notes.md"
if python3 - "$r/latest.json" "$version" << 'PY'; then pass "release notes land in the manifest and change nothing else"; else bad "release notes land in the manifest and change nothing else"; fi
import json, sys
m = json.load(open(sys.argv[1]))
assert m["notes"].startswith("## What is new"), m["notes"]
assert m["version"] == sys.argv[2]
assert all(e["signature"] for e in m["platforms"].values())
PY
expect_ok "a manifest with real notes still verifies" "$r"

# What GitHub generates for a release, with a new contributor and the changelog link.
cat > "$work/github-notes.md" << 'MD'
## What's Changed
* FIG-81: site and README for v0.2.0 by @someone in https://github.com/figlock/figlock/pull/127
* fix(cli): keep "travel by @air" in a title by @someone-else in https://github.com/figlock/figlock/pull/129
* feat(desktop): in-app update by @someone in https://github.com/figlock/figlock/pull/200

## New Contributors
* @newperson made their first contribution in https://github.com/figlock/figlock/pull/130

**Full Changelog**: https://github.com/figlock/figlock/compare/v0.2.0...v0.3.0
MD
make_release
"$here/set-update-notes.sh" "$r/latest.json" "$work/github-notes.md"
expected=$'- FIG-81: site and README for v0.2.0 (#127)\n- fix(cli): keep "travel by @air" in a title (#129)\n- feat(desktop): in-app update (#200)'
if python3 - "$r/latest.json" "$expected" << 'PY'; then pass "GitHub's generated notes become plain titles with PR numbers"; else bad "GitHub's generated notes become plain titles with PR numbers"; fi
import json, sys
notes = json.load(open(sys.argv[1]))["notes"]
assert notes == sys.argv[2], repr(notes)
PY
expect_ok "a manifest with trimmed notes still verifies" "$r"

make_release
python3 - "$work/many.md" << 'PY'
import sys
open(sys.argv[1], "w").write("## What's Changed\n" + "".join(f"* change number {i} with a fairly long title to fill space by @a in https://github.com/x/y/pull/{i}\n" for i in range(400)))
PY
"$here/set-update-notes.sh" "$r/latest.json" "$work/many.md"
if python3 - "$r/latest.json" << 'PY'; then pass "long notes are cut at a whole line"; else bad "long notes are cut at a whole line"; fi
import json, sys, re
notes = json.load(open(sys.argv[1]))["notes"]
assert len(notes) <= 2000, len(notes)
body = notes.rstrip("…").split("\n")
assert all(re.match(r"^- change number \d+ with a fairly long title to fill space \(#\d+\)$", l) for l in body), body[-1]
PY

head -c 5000 /dev/zero | tr '\0' 'a' > "$work/long.md"
"$here/set-update-notes.sh" "$r/latest.json" "$work/long.md"
if python3 -c "import json;n=json.load(open('$r/latest.json'))['notes'];assert len(n)<=2000,len(n)"; then pass "over-long notes are cut"; else bad "over-long notes are cut"; fi

: > "$work/empty.md"
"$here/set-update-notes.sh" "$r/latest.json" "$work/empty.md" 2> /dev/null
if python3 -c "import json;assert json.load(open('$r/latest.json'))['notes'].startswith('aaa')"; then pass "empty notes leave the manifest as it was"; else bad "empty notes leave the manifest as it was"; fi

echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
