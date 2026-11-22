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

# Tests for the release scripts (needs minisign): a good release passes and each broken one fails.
release-scripts-test:
    scripts/test-release-scripts.sh

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
ios-test: ios-xcframework
    cd apps/ios/Shared && xcodebuild test -scheme Shared -destination 'platform=iOS Simulator,name=iPhone 17' -derivedDataPath ../build

# iOS: build the app for the simulator.
ios-build: ios-xcframework ios-project
    cd apps/ios && xcodebuild -project FigLock.xcodeproj -scheme FigLock -destination 'generic/platform=iOS Simulator' -derivedDataPath build CODE_SIGNING_ALLOWED=NO build

# iOS: build, install and launch the app on a booted simulator (boots "iPhone 17" when none is running).
ios-run: ios-build
    apps/ios/scripts/run-simulator.sh
