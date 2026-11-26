#!/usr/bin/env bash
# Print the id of the simulator to test on: "iPhone 17" when this Xcode has it, else the first available iPhone.
set -euo pipefail

list="$(xcrun simctl list devices available | grep -E '^ +iPhone ')"
line="$(grep -E '^ +iPhone 17 \(' <<<"$list" | head -1 || true)"
[ -n "$line" ] || line="$(head -1 <<<"$list")"
[ -n "$line" ] || { echo "no iPhone simulator available" >&2; exit 1; }
sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/' <<<"$line"
