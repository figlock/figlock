#!/usr/bin/env bash
# Print the id of the simulator to test on. With no argument: "iPhone 17" when this Xcode has it, else the first
# available iPhone. With `ipad`: "iPad Pro 11-inch" when this Xcode has it, else the first available iPad.
# `SIMULATOR_ID` overrides both (a person or CI picking an exact device).
set -euo pipefail

[ -z "${SIMULATOR_ID:-}" ] || { echo "$SIMULATOR_ID"; exit 0; }

case "${1:-iphone}" in
  iphone) family="iPhone"; preferred="iPhone 17" ;;
  ipad) family="iPad"; preferred="iPad Pro 11-inch" ;;
  *) echo "usage: simulator-id.sh [iphone|ipad]" >&2; exit 2 ;;
esac

list="$(xcrun simctl list devices available | grep -E "^ +$family " || true)"
line="$(grep -E "^ +$preferred" <<<"$list" | head -1 || true)"
[ -n "$line" ] || line="$(head -1 <<<"$list")"
[ -n "$line" ] || { echo "no $family simulator available" >&2; exit 1; }
sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/' <<<"$line"
