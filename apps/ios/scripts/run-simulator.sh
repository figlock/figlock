#!/usr/bin/env bash
# Install and launch the simulator build of FigLock on one device: `run-simulator.sh [iphone|ipad]` (default iphone).
# The device is the one simulator-id.sh picks ("iPhone 17" or "iPad Pro 11-inch"), or SIMULATOR (a name or an id).
# It boots that device when needed and never touches another running simulator.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
device="${SIMULATOR:-$("$root/scripts/simulator-id.sh" "${1:-iphone}")}"
app="$root/build/Build/Products/Debug-iphonesimulator/FigLock.app"

xcrun simctl boot "$device" 2>/dev/null || true
# Newer Xcode versions ship no separate Simulator.app; the device runs headless and `simctl io` can screenshot it.
sim_app="$(xcode-select -p)/Applications/Simulator.app"
[ -d "$sim_app" ] && open "$sim_app"
xcrun simctl bootstatus "$device" -b >/dev/null
xcrun simctl install "$device" "$app"
xcrun simctl launch "$device" com.figlock.ios
