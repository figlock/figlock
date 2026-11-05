#!/usr/bin/env bash
# Phase 2 exit test (FIG-39): a real Claude Code session uses the figlock skill to call GitHub through the
# broker, and the token never shows up anywhere the agent or a log could hold it (threat model G11).
#
# Run it yourself in a terminal on a Mac. It asks once for a GitHub token (hidden prompt; use a fine-grained,
# read-only, short-lived one) and you approve two Touch ID prompts. The vault, broker and logs live in a
# temporary folder that is deleted at the end. Nothing here prints the token.
#
#   scripts/e2e/agent-github.sh
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
bin=${FIGLOCK_BIN:-$repo/target/debug/figlock}
[ -x "$bin" ] || (cd "$repo" && cargo build -q -p figlock-cli)
command -v claude >/dev/null || { echo "claude (Claude Code) is not on PATH"; exit 2; }

work=$(mktemp -d /tmp/fl39.XXXXXX) # short: the broker socket path has a length limit
home=$work/home
store=$work/store
agent=$work/agent
mkdir -p "$store" "$agent/bin"
cp "$bin" "$agent/bin/figlock"
broker_pid=""
pat=""

cleanup() {
  FIGLOCK_HOME=$home "$agent/bin/figlock" broker lock >/dev/null 2>&1 || true
  [ -n "$broker_pid" ] && kill "$broker_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

# The throwaway vault's own password is random and only ever in this script's environment.
pw=$(openssl rand -hex 16)
fl() { FIGLOCK_HOME=$home "$agent/bin/figlock" "$@"; }

printf 'GitHub token (hidden, read-only): '
read -rs pat
printf '\n'
[ -n "$pat" ] || { echo "no token given"; exit 2; }

FIGLOCK_PASSWORD=$pw fl init --store "$store" --kdf floor >"$work/init.txt"
sk=$(sed -n 's/^Secret Key: //p' "$work/init.txt")
rm "$work/init.txt"
printf '%s\n' "$pat" | FIGLOCK_PASSWORD=$pw FIGLOCK_SECRET_KEY=$sk fl add --type token \
  --title "GitHub e2e" --set provider=github --set allowed_hosts/a1=api.github.com --secret token >/dev/null

start_broker() { # [--deny-all]
  FIGLOCK_PASSWORD=$pw FIGLOCK_SECRET_KEY=$sk fl broker start "$@" >>"$work/broker.log" 2>&1 &
  broker_pid=$!
  for _ in $(seq 50); do [ -S "$home/run/broker.sock" ] && return; sleep 0.2; done
  echo "the broker did not start"; cat "$work/broker.log"; exit 1
}
stop_broker() { fl broker lock >/dev/null; wait "$broker_pid" 2>/dev/null || true; broker_pid=""; }

# One headless Claude Code session in a folder that only has the figlock skill and the figlock binary.
# It gets no vault password, no Secret Key and no token in its environment.
(cd "$agent" && "$agent/bin/figlock" skill install --project >/dev/null)
session() { # name prompt
  (cd "$agent" && env -u FIGLOCK_PASSWORD -u FIGLOCK_SECRET_KEY FIGLOCK_HOME=$home PATH="$agent/bin:$PATH" \
    claude -p "$2" --allowedTools "Bash(figlock:*)" --output-format stream-json --verbose \
    >"$work/$1.jsonl" 2>"$work/$1.err" </dev/null) || true
}

# What the agent ran and what it got back, for the checks below. Prints "<kind>\t<text>" lines.
digest() {
  python3 - "$1" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    d = json.loads(line)
    for c in (d.get("message") or {}).get("content") or []:
        if not isinstance(c, dict):
            continue
        if c.get("type") == "tool_use":
            print("RAN\t" + str(c["input"].get("command", c["input"])))
        elif c.get("type") == "tool_result":
            print("GOT\t" + str(c["content"]).replace("\n", " "))
        elif c.get("type") == "text":
            print("SAID\t" + c["text"].replace("\n", " "))
PY
}

fail=0
check() { # description, command...
  local d=$1; shift
  if "$@"; then echo "  ok    $d"; else echo "  FAIL  $d"; fail=1; fi
}
saw() { digest "$work/$1.jsonl" >"$work/$1.digest"; grep -q "^GOT.*$2" "$work/$1.digest"; } # session, text in a tool result
not() { ! "$@"; }
count_ran() { digest "$work/$1.jsonl" | grep -c $'^RAN\t.*figlock http' || true; }

echo "1. denied approval: the agent must not retry"
start_broker --deny-all
session denied 'Using my FigLock item titled "GitHub e2e", GET https://api.github.com/user and tell me the login.'
stop_broker
check "the agent tried figlock http" test "$(count_ran denied)" -ge 1
check "it did not retry after the denial" test "$(count_ran denied)" -le 1
check "the denial was reported" saw denied 'not approved' 

echo "2. approved call to GitHub (approve the Touch ID prompt)"
start_broker
session allowed 'Using my FigLock item titled "GitHub e2e", GET https://api.github.com/user and tell me the login.'
check "the agent used figlock http" test "$(count_ran allowed)" -ge 1
check "GitHub answered (a login came back)" saw allowed '"login"' 

echo "3. host off the allow-list (approve the Touch ID prompt)"
session offhost 'Using my FigLock item titled "GitHub e2e", GET https://example.com/ and tell me the page title.'
stop_broker
check "the request was refused" saw offhost 'not.allowed'
check "no page came back" not saw offhost 'Example Domain' 

echo "4. the token is nowhere (transcripts, stderr, broker log, vault home, storage)"
scan() { grep -rlF -f <(printf '%s\n' "$pat") "$work" 2>/dev/null || true; }
check "exact token not found in any file" test -z "$(scan)"
shape='gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}'
check "no GitHub token-shaped string in any file" bash -c "! grep -rEq '$shape' '$work'"
check "no Authorization header value in any transcript" bash -c "! grep -qiE 'bearer [A-Za-z0-9_-]{20,}' '$work'/*.jsonl"

echo
for s in denied allowed offhost; do echo "--- $s: what the agent ran and saw"; digest "$work/$s.jsonl" | cut -c1-220; done
echo
[ "$fail" = 0 ] && echo "PASS" || { echo "FAIL"; exit 1; }
