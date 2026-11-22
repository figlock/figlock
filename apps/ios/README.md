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

## Local vault

`Shared` owns the one local vault both targets use (ADR-0041 decision 2, ADR-0026). `VaultContainer` lays out the
App Group container: `vault/` (ciphertext: `vault.json`, `devices/`, `log/`) and `device/` (this phone's own
state). Both folders are excluded from backups and use the `completeUntilFirstUserAuthentication` protection
class. `LocalVault` opens the folder through the Rust folder store (atomic, create-only writes; path rules shared
with every platform): the app with `.readWrite`, the extension with `.readOnly`, which cannot write or create
anything. `status()` tells onboarding (`noVault`), a damaged `vault.json`, an unreadable file and the recovery
screen case (`missing`) apart; a folder that vanished is never recreated empty. The device-only check of the
protection class belongs to the on-device tests (FIG-172); the simulator does not report it.

## Unlock

`Shared/Sources/Shared/Unlock` holds the whole unlock layer, with the hardware parts behind protocols so tests run
on the simulator with a software key. `QuickUnlockChip` is the Secure Enclave key (`SecureEnclaveChip`: permanent,
shared keychain group, `.userPresence`, `ThisDeviceOnly`); the wrap of the Account Unlock Key is the Rust code of
`figlock-core`. `QuickUnlockPolicy` enforces the limits of `crypto.md` 8 and `PasswordThrottle` slows password
guesses. `SecretKeyKeeper` keeps the Secret Key and the device keys in the keychain (`ThisDeviceOnly`, never
synced) and remembers the ADR-0038 choice to stop keeping the key. `AppLock` ties them together for the app.
The simulator has no Secure Enclave, so Face ID is off there. Debug builds accept `-figlock-seed-demo` to create a
made-up vault (master password `demo-password-1`) so the unlock screen can be run before onboarding exists.

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

## Theme

`Shared/Sources/Shared/Theme/Tokens.generated.swift` is generated from `design/tokens.css` by
`python3 design/gen-swift-theme.py` and is committed; `just design-check` (CI) fails when the two differ. Use
`FLColor`, `FLSpace`, `FLRadius`, `FLText`, `FLSize`, `FLShadow` and `FLDuration` rather than literals, `FLFont.ui`
and `FLFont.mono` for text (SF Pro and SF Mono, scaled with Dynamic Type), `.flMinTouchTarget()` for 44 pt hit areas
and `.flAnimation(value:)` or `FLMotion.animation` so Reduce Motion drops the movement.

## Signing

Simulator builds need no signing. The bundle ids (`com.figlock.ios`, `com.figlock.ios.autofill`), App Group and
keychain group in the entitlements are placeholders until the Apple Developer ids exist (FIG-44, FIG-175).

## CI

The `ios` job in `.github/workflows/ci.yml` runs on a macOS runner only when `apps/ios`, `figlock-core`,
`figlock-ffi` or `figlock-sync` change (the other paths that matter are the lockfile, `mise.toml`, `justfile` and the
workflow). It runs `just ios-lint`, `just ios-test` and `just ios-build`. The required check in the main ruleset is
the aggregate `ci` job, which counts a skipped `ios` job as passing, so no ruleset change is needed.
