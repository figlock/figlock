# Run `just` with no arguments to list recipes.
default:
    @just --list

# One-time setup: install pinned tools and enable the git hooks.
setup:
    mise install
    git config core.hooksPath .githooks

# Format all code.
fmt:
    if [ -f Cargo.toml ]; then cargo fmt --all; fi
    if [ -f pnpm-workspace.yaml ]; then pnpm -r --if-present run fmt; fi

# Lint all code. Warnings are errors.
lint:
    if [ -f Cargo.toml ]; then cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --workspace --all-targets --all-features -- -D warnings; fi
    if [ -f pnpm-workspace.yaml ]; then pnpm -r --if-present run lint; fi

# Run all tests.
test:
    if [ -f Cargo.toml ]; then cargo test --workspace --all-features; fi
    if [ -f pnpm-workspace.yaml ]; then pnpm -r --if-present run test; fi

# Regenerate the test vectors in docs/spec/vectors (only when a spec rule changes).
vectors:
    cd docs/spec/vectors && python3 -m venv .venv && .venv/bin/pip install -q -r requirements.txt && .venv/bin/python generate.py

# Check committed test vectors for drift and re-verify every case.
vectors-check:
    cd docs/spec/vectors && python3 -m venv .venv && .venv/bin/pip install -q -r requirements.txt && .venv/bin/python generate.py --check

# WCAG contrast check of the design tokens, and the site copy of them has not drifted.
design-check:
    python3 -I design/check-contrast.py
    cmp design/tokens.css site/tokens.css
    python3 -I design/gen-swift-theme.py --check
    python3 -I -m unittest discover -s design

# Tests for the release scripts (needs minisign): a good release passes and each broken one fails.
release-scripts-test:
    scripts/test-release-scripts.sh
    scripts/test-ios-version.sh

# Everything CI runs: lint, tests, the design token check, the vector drift check and the release script tests.
check: lint test design-check vectors-check release-scripts-test

# Supply-chain checks: advisories, licenses, bans, sources.
deny:
    if [ -f Cargo.toml ]; then cargo deny check; fi

# iOS: cross-compile figlock-ffi, generate the Swift bindings, build Shared/Frameworks/FigLockFFI.xcframework.
ios-xcframework:
    apps/ios/scripts/build-xcframework.sh

# iOS: generate apps/ios/FigLock.xcodeproj from project.yml (the project is never hand edited).
ios-project:
    cd apps/ios && xcodegen generate --quiet

# iOS: run the Shared package tests on a simulator (needs the xcframework, which has no macOS slice).
# The extension memory test measures the footprint of the whole process, so it runs alone: other tests (Argon2id
# in the onboarding ones) allocate at the same time and would fail it for memory it did not use.
ios-test: ios-xcframework
    cd apps/ios/Shared && xcodebuild test -scheme Shared -destination 'platform=iOS Simulator,id='"$({{justfile_directory()}}/apps/ios/scripts/simulator-id.sh)" -derivedDataPath ../build -skip-testing:'SharedTests/replayingTenThousandItemsInTheExtensionStaysInsideTheMemoryLimit()'
    cd apps/ios/Shared && xcodebuild test -scheme Shared -destination 'platform=iOS Simulator,id='"$({{justfile_directory()}}/apps/ios/scripts/simulator-id.sh)" -derivedDataPath ../build -only-testing:'SharedTests/replayingTenThousandItemsInTheExtensionStaysInsideTheMemoryLimit()'

# iOS: SwiftLint and swift-format (strict). Generated bindings are excluded. Run `just ios-fmt` to fix formatting.
ios-lint:
    cd apps/ios && swiftlint lint --strict --quiet
    cd apps/ios && xcrun swift-format lint --strict --recursive App AutofillExtension Shared/Sources/Shared Shared/Tests UITests Shared/Package.swift

# iOS: format the Swift sources in place.
ios-fmt:
    cd apps/ios && xcrun swift-format format --in-place --recursive App AutofillExtension Shared/Sources/Shared Shared/Tests UITests Shared/Package.swift

# iOS: UI tests on the simulator against the demo vault (needs the xcframework and project)
ios-uitest: ios-xcframework ios-project
    cd apps/ios && xcodebuild test -project FigLock.xcodeproj -scheme FigLock -destination 'platform=iOS Simulator,id='"$({{justfile_directory()}}/apps/ios/scripts/simulator-id.sh)" -derivedDataPath build

# iOS: build the app for the simulator.
ios-build: ios-xcframework ios-project
    cd apps/ios && xcodebuild -project FigLock.xcodeproj -scheme FigLock -destination 'generic/platform=iOS Simulator' -derivedDataPath build CODE_SIGNING_ALLOWED=NO build

# iOS: build the app for an iPad simulator ("iPad Pro 11-inch"), so a broken iPad build shows up (ADR-0048). Signed
# for the simulator (no CODE_SIGNING_ALLOWED=NO): without the App Group entitlement the app shows "Storage unavailable".
ios-build-ipad: ios-xcframework ios-project
    cd apps/ios && xcodebuild -project FigLock.xcodeproj -scheme FigLock -destination 'platform=iOS Simulator,id='"$({{justfile_directory()}}/apps/ios/scripts/simulator-id.sh ipad)" -derivedDataPath build build

# iOS: build, install and launch the app on the "iPhone 17" simulator (booted if needed, other simulators untouched).
ios-run: ios-build
    apps/ios/scripts/run-simulator.sh iphone

# iOS: build, install and launch the app on the "iPad Pro 11-inch" simulator.
ios-run-ipad: ios-build-ipad
    apps/ios/scripts/run-simulator.sh ipad
