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
made-up vault (master password `demo-password-1`) so the unlock screen can be run before onboarding exists, and
`-figlock-unlock-demo` also unlocks it so the vault screens can be looked at in a simulator.

## Vault screens

`Shared/Sources/Shared/Vault` is the read path. `VaultLog` applies the op files under `log/` (the clock is read per
file, because the vault hybrid clock counts ops at one instant); loading and index building run off the main
thread. `VaultBrowser` holds the list, filters and fuzzy search (under 100 ms at 10,000 items, measured in a test).
`Clipboard` copies with a 30 s auto-clear and a local-only, expiring pasteboard item; it clears only while the
pasteboard still holds that copy. `SecretReveal` gates reveal behind Face ID or the passcode and masks again after
10 s (`design/DESIGN.md` 8); leaving the detail view or the foreground cancels a pending reveal. Copying a secret
needs no extra check (the vault is unlocked), revealing does. Face ID and the real pasteboard do not run on the
simulator. The views are in `App/Vault`.

## Editing items

`ItemDraft` is what the editor works on (all six kinds, document files included); `changes()` turns it into the
field writes of the session, and a stored secret is never put into a field: it shows as "unchanged" until
something is typed. `VaultEditor` runs create, update, delete and restore through `VaultSession`, commits one op
file per save and writes it to the local vault folder; if the write fails the commit is rolled back and nothing
changes. `PendingSync` remembers which items were changed here, and an item shows as "not synced" until
`FileSyncState.lastSyncMs` (the time a sync started) is later than its edit. Document files (ADR-0024: 8 files, 768 KB
each) come from the Files picker through `DocumentFiles` and go back out through `FileExtraction` (Face ID first). The generator is the
Rust one (`GeneratorSettings`). Views: `App/Vault/ItemFormView.swift`, `GeneratorView.swift`, `ItemManagement.swift`.

## Onboarding

`Shared/Sources/Shared/Onboarding` gets a vault onto the phone. `Onboarding` creates one (nothing is stored until the
Emergency Kit check passes), joins one by pairing through the remote's `inbox/` (`Onboarding.Pairing`), opens one from
Google Drive and restores a missing folder from it, and removes a vault from the phone. The remote is any
`BlobStoreFfi`; Drive is the Rust `DriveStore` fed tokens by `GoogleAccessTokens` (PKCE sign-in, refresh token in the
keychain). The Google client id comes from the build (`FIGLOCK_GOOGLE_CLIENT_ID`, Info.plist `FigLockGoogleClientID`)
and is empty until the iOS OAuth client exists (FIG-44), in which case the screens say sign-in is not set up. The
SwiftUI screens are in `App/Onboarding` and `App/Onboarding/RecoveryScreen.swift`.

## Settings

`Shared/Sources/Shared/Settings` is the logic behind Settings, the screens are in `App/Settings` (the gear in the
vault list). `DeviceSettings` keeps this phone's auto-lock time (5 minutes by default, no "never") and device
nicknames in the device folder. `SettingsService` revokes a device (`VaultSession.revokeDevice` in the Rust core,
ADR-0018: needs the master password and the Secret Key, which the vault checks; the new files are written snapshot,
records, `vault.json` last, then pushed when this phone is signed in to Drive), renders the Emergency Kit again from
the kept Secret Key (`SecretKeyReveal`: Face ID or passcode first, the key masks after 10 s), and deletes the Drive
copy only after the typed phrase of ADR-0035. Signing out of Google and removing the vault from this phone never
delete the Drive copy. A device rename is a nickname shown on this iPhone only: renaming it for every device means
re-signing its record outside a key rotation, which `sync.md` does not allow and needs an ADR first.

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
