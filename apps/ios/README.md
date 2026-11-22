# FigLock for iPhone

SwiftUI app plus an AutoFill Credential Provider extension over the Rust core (ADR-0041). iPhone only, iOS 18
or later.

```
App/                  the app target
AutofillExtension/    the credential provider extension (placeholder until FIG-166)
Shared/               Swift package wrapping the generated bindings; used by both targets
scripts/              build-xcframework.sh, run-simulator.sh
project.yml           XcodeGen spec; the .xcodeproj is generated and never committed or hand edited
```

## Build and run

Needs Xcode with an iOS simulator runtime and the tools pinned in `mise.toml` (`just setup`; it also installs the
two Rust iOS targets and XcodeGen).

```sh
just ios-xcframework   # cross-compile figlock-ffi (device + Apple silicon simulator), generate bindings
just ios-project       # generate FigLock.xcodeproj
just ios-test          # Shared package tests on a simulator
just ios-lint          # SwiftLint and swift-format (strict); `just ios-fmt` fixes formatting
just ios-run           # build, then install and launch on a simulator (SIMULATOR="iPhone 17" by default)
```

The xcframework and the Swift bindings (`Shared/Frameworks`, `Shared/Sources/FigLockFFI`) are generated from the
Rust sources every time and are gitignored, so they cannot drift from the core. There is no Intel simulator
slice.

## Signing

Simulator builds need no signing. The bundle ids (`com.figlock.ios`, `com.figlock.ios.autofill`), App Group and
keychain group in the entitlements are placeholders until the Apple Developer ids exist (FIG-44, FIG-175).

## CI

The `ios` job in `.github/workflows/ci.yml` runs on a macOS runner only when `apps/ios`, `figlock-core`,
`figlock-ffi` or `figlock-sync` change (the other paths that matter are the lockfile, `mise.toml`, `justfile` and the
workflow). It runs `just ios-lint`, `just ios-test` and `just ios-build`. The required check in the main ruleset is
the aggregate `ci` job, which counts a skipped `ios` job as passing, so no ruleset change is needed.
