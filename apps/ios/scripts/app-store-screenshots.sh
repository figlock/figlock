#!/usr/bin/env bash
# Retake the App Store screenshots of the screens that need taps (FIG-174), on an iPhone 6.9 inch simulator, light mode,
# 9:41 status bar, made-up demo vault. The UI test navigates and waits; this script takes each picture with simctl,
# because the app hides itself from in-process screenshots. The static screens (welcome, lock, vault) are in
# design/prototypes/ios/app-store/ too. Usage: apps/ios/scripts/app-store-screenshots.sh [simulator name or id]
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
device="${1:-iPhone 18 Pro Max}"
out="$root/../../design/prototypes/ios/app-store"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

xcrun simctl boot "$device" 2>/dev/null || true
xcrun simctl bootstatus "$device" -b > /dev/null
xcrun simctl ui "$device" appearance light
xcrun simctl status_bar "$device" override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
trap 'xcrun simctl status_bar "$device" clear; rm -rf "$work"' EXIT

(cd "$root" && TEST_RUNNER_FIGLOCK_SCREENSHOT_DIR="$work" xcodebuild test -project FigLock.xcodeproj -scheme FigLock \
  -destination "platform=iOS Simulator,name=$device" -derivedDataPath "$root/build" \
  -only-testing:FigLockUITests/AppStoreScreenshotsUITests CODE_SIGN_IDENTITY=- > "$work/xcodebuild.log" 2>&1) &
test_pid=$!

while kill -0 "$test_pid" 2> /dev/null; do
  for ready in "$work"/*.ready; do
    [ -e "$ready" ] || continue
    name="$(basename "$ready" .ready)"
    sleep 1
    xcrun simctl io "$device" screenshot "$out/$name-light.png" > /dev/null 2>&1
    rm "$ready"
    touch "$work/$name.done"
    echo "captured $name"
  done
  sleep 0.3
done
wait "$test_pid" || { tail -30 "$work/xcodebuild.log"; exit 1; }
