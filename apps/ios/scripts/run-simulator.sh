#!/usr/bin/env bash
# Install and launch the simulator build of FigLock. Boots the named device when none is booted.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
device="${SIMULATOR:-iPhone 17}"
app="$root/build/Build/Products/Debug-iphonesimulator/FigLock.app"

if ! xcrun simctl list devices booted | grep -q Booted; then
  xcrun simctl boot "$device"
fi
# Newer Xcode versions ship no separate Simulator.app; the device runs headless and `simctl io` can screenshot it.
sim_app="$(xcode-select -p)/Applications/Simulator.app"
[ -d "$sim_app" ] && open "$sim_app"
xcrun simctl install booted "$app"
xcrun simctl launch booted com.figlock.ios
