#!/usr/bin/env bash
# Cross-compile figlock-ffi for iPhone (device and simulator), generate the Swift bindings from the built
# library, and package the static libraries as Shared/Frameworks/FigLockFFI.xcframework. Nothing generated is
# committed (see apps/ios/.gitignore). Run from anywhere: `just ios-xcframework`.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
shared="$root/apps/ios/Shared"
profile="${PROFILE:-release}"
out="$root/target/ios-xcframework"
targets=(aarch64-apple-ios aarch64-apple-ios-sim)
# Keep the deployment target in step with project.yml.
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-18.0}"

cd "$root"
flags=()
[ "$profile" = release ] && flags+=(--release)

for t in "${targets[@]}"; do
  cargo build -p figlock-ffi "${flags[@]}" --target "$t"
done

# The bindings come from the metadata in a library of the host, the same way the other platforms do it.
cargo build -p figlock-ffi --features bindgen
host_lib="$root/target/debug/libfiglock_ffi.dylib"
rm -rf "$out" && mkdir -p "$out/bindings" "$out/headers"
cargo run -q -p figlock-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "$host_lib" --language swift --out-dir "$out/bindings"

# Swift source into the package; the C header and module map into the xcframework.
mkdir -p "$shared/Sources/FigLockFFI" "$shared/Frameworks"
cp "$out/bindings/figlock_ffi.swift" "$shared/Sources/FigLockFFI/figlock_ffi.swift"
cp "$out/bindings/figlock_ffiFFI.h" "$out/headers/"
cp "$out/bindings/figlock_ffiFFI.modulemap" "$out/headers/module.modulemap"

rm -rf "$shared/Frameworks/FigLockFFI.xcframework"
args=()
for t in "${targets[@]}"; do
  args+=(-library "$root/target/$t/$profile/libfiglock_ffi.a" -headers "$out/headers")
done
xcodebuild -create-xcframework "${args[@]}" -output "$shared/Frameworks/FigLockFFI.xcframework"
echo "built $shared/Frameworks/FigLockFFI.xcframework"
