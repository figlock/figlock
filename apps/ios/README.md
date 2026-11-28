# FigLock for iPhone and iPad

SwiftUI app plus an AutoFill Credential Provider extension over the Rust core (ADR-0041). One universal build
for iPhone and iPad (ADR-0048), iOS 18 or later.

```
App/                  the app target
AutofillExtension/    the credential provider extension (read-only, Face ID only, ADR-0041); with no login matching the site it lists
                      every login newest first under "No saved login for <site>", with search
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
shared keychain group, `.biometryCurrentSet`, `ThisDeviceOnly`, ADR-0046); the wrap of the Account Unlock Key is the Rust code of
`figlock-core`. `QuickUnlockPolicy` enforces the limits of `crypto.md` 8 and `PasswordThrottle` slows password
guesses. `SecretKeyKeeper` keeps the Secret Key and the device keys in the keychain (`ThisDeviceOnly`, never
synced) and remembers the ADR-0038 choice to stop keeping the key. `AppLock` ties them together for the app.
The simulator has no Secure Enclave, so Face ID is off there. Debug builds accept `-figlock-seed-demo` to create a
made-up vault (password `demo-password-1`) so the unlock screen can be run before onboarding exists (it also
clears any saved password pause, so a run that tripped it does not affect the next), and
`-figlock-unlock-demo` also unlocks it so the vault screens can be looked at in a simulator. `-figlock-fresh`
removes any vault from the phone first, so a UI test starts at onboarding.

## Vault screens

`Shared/Sources/Shared/Vault` is the read path. `VaultLog` applies the op files under `log/` (the clock is read per
file, because the vault hybrid clock counts ops at one instant); loading and index building run off the main
thread. `VaultBrowser` holds the list, filters and fuzzy search (under 100 ms at 10,000 items, measured in a test).
`Clipboard` copies with a 30 s auto-clear and a local-only, expiring pasteboard item; it clears only while the
pasteboard still holds that copy. `SecretReveal` gates reveal behind Face ID or the passcode and masks again after
10 s (`design/DESIGN.md` 8); leaving the detail view or the foreground cancels a pending reveal. Copying a secret
needs no extra check (the vault is unlocked), revealing does. Face ID and the real pasteboard do not run on the
simulator. The views are in `App/Vault`.

## Export

Settings > Export saves the vault as the JSON `figlock export` writes (`figlock_core::export_json`, through
`VaultSession.exportJson`). Without secrets it asks nothing; with them `VaultExporter` asks for Face ID or the passcode
every time and makes no file when that is refused. The file is a temporary one handed to the share sheet (Copy is
excluded, as for the Emergency Kit) and removed when the sheet closes, the screen goes away or the app goes to the background. Debug builds take
`-figlock-allow-device-auth` and `-figlock-deny-device-auth` so the UI tests do not depend on simulator biometrics.

## Forgot password

The lock screen's "I forgot my password or Secret Key" opens `ForgotPasswordView`: Recovery Key code, new
password, new Emergency Kit, then the check of the new Secret Key. It is the desktop's Forgot flow over the same Rust
`Vault::recover` (`start_recovery` in `figlock-ffi`); `Onboarding.startRecovery` / `prepareRecovery` /
`commitRecovery` hold it. Nothing is written until the kit check passes; the commit replaces `vault.json`, drops the
kept Secret Key, Face ID's blob and any password pause, and the app then unlocks with the new credentials. The old
Recovery Key keeps working (the phone does not make a new one). Recovery of a vault that is only
in Drive are not on the phone yet. Debug builds accept `-figlock-seed-recovery` (a throwaway vault with a made-up
Recovery Key, `RecoveryFixture`) for the UI test. If the Recovery Key is lost, the code step offers "Remove this
vault from this iPhone" (with a confirmation): the same removal as Settings (`Onboarding.removeFromThisPhone`, Drive
sign out, device settings cleared, `lock.vaultDidChange`), landing on the vault setup screen and never touching the
copies in the sync places.

## Editing items

`ItemDraft` is what the editor works on (all six kinds, document files included); `changes()` turns it into the
field writes of the session, and a stored secret is never put into a field: it shows as "unchanged" until
something is typed. `VaultEditor` runs create, update, delete and restore through `VaultSession`, commits one op
file per save and writes it to the local vault folder; if the write fails the commit is rolled back and nothing
changes. `PendingSync` remembers which items were changed here, and an item shows as "not synced" until
`FileSyncState.lastSyncMs` (the time a sync started) is later than its edit. Document files (ADR-0024: 8 files,
768 KB each) come from the Files picker through `DocumentFiles` and go back out through `FileExtraction` (Face ID
first). Tags are chips with a remove button (`TagsField`) over the `ItemDraft.tags` rules. Website is what AutoFill
matches on, and the editor says so. The generator is the Rust one (`GeneratorSettings`). Views: `App/Vault/ItemFormView.swift`,
`GeneratorView.swift`, `ItemManagement.swift`.

## Onboarding

`Shared/Sources/Shared/Onboarding` gets a vault onto the phone. `Onboarding` creates one (nothing is stored until the
Emergency Kit check passes), joins one by pairing through the remote's `inbox/` (`Onboarding.Pairing`), opens one from
Google Drive and restores a missing folder from it, and removes a vault from the phone. The remote is any
`BlobStoreFfi`; Drive is the Rust `DriveStore` fed tokens by `GoogleAccessTokens` (PKCE sign-in, refresh token in the
keychain, and the account email from the Drive `about` call, kept beside it, cleared on sign out, shown on the Sync
screens and never logged; no scope beyond `drive.appdata`). The Google client id comes from the build (`FIGLOCK_GOOGLE_CLIENT_ID`, Info.plist `FigLockGoogleClientID`)
and is the iOS-type OAuth client in the `figlock-prod` project (bundle id `com.figlock.ios`, no secret). A build with it empty says sign-in is not set up. The
SwiftUI screens are in `App/Onboarding` and `App/Onboarding/RecoveryScreen.swift`.

## Settings

`Shared/Sources/Shared/Settings` is the logic behind Settings, the screens are in `App/Settings` (the gear in the
vault list). `DeviceSettings` keeps this phone's auto-lock time (5 minutes by default, no "never"), the appearance
(System, Light or Dark; System by default, applied app-wide with `preferredColorScheme`) and device
nicknames in the device folder. `SettingsService` revokes a device (`VaultSession.revokeDevice` in the Rust core,
ADR-0018: needs the password and the Secret Key, which the vault checks; the new files are written snapshot,
records, `vault.json` last, then pushed when this phone is signed in to Drive), renders the Emergency Kit again from
the kept Secret Key (`SecretKeyReveal`: Face ID or passcode first, the key masks after 10 s), and deletes the Drive
copy only after the typed phrase of ADR-0035. Signing out of Google and removing the vault from this phone never
delete the Drive copy. A device rename is a nickname shown on this iPhone only: renaming it for every device means
re-signing its record outside a key rotation, which `sync.md` does not allow and needs an ADR first.

Settings > Recovery Key (ADR-0029, the same rules as the desktop) shows whether the vault has one, turns it on or
replaces it, and turns it off. `SettingsService.setRecoveryKey` takes a `RecoveryDraft` (`new_recovery_draft` in
`figlock-ffi`: the code is made in memory, shown once, with a one page PDF to save, and checked by typing its last two
groups) and, after the password and the Secret Key (kept or typed) were checked by the vault
(`VaultSession.setRecoveryKey`), writes the new `vault.json` and syncs it. Replacing makes the old code open nothing
from the new `vault.json` on. `removeRecoveryKey` proves the same credentials against the stored `vault.json` first,
because dropping the key needs no secret of the vault. A wrong password or Secret Key changes nothing; if the write
itself fails after the vault took the change, the app locks so the next unlock reads what is really stored. FigLock
never keeps the code and never shows an old one again. The sheets (`RecoveryKeySheets.swift`) close right after the
vault change and the sync runs afterwards. The UI test (`RecoveryKeySettingsUITests`) runs on the demo
vault.

Settings > Security > Password health (`App/Settings/PasswordHealthView.swift`, logic in `Shared/Vault/PasswordHealth.swift`)
is the same report as the Mac Health page: weak, reused and old passwords come from `figlock-core::health` through
`VaultSession.healthReport`, and each item that needs attention opens its item. The breach check is opt-in per run
(`startBreachCheck`, `BreachCheck.finish`): the app does the Have I Been Pwned range request itself (`URLSession`, only the
5 character SHA-1 prefix leaves the phone, no redirects, a size cap, `Add-Padding`). If the service cannot be reached the
screen says so and keeps the other results, and one failed prefix fails the whole check so a half-checked vault is never
shown as clean. Debug builds accept `-figlock-seed-health` (a made-up vault with weak, reused and old items) and
`-figlock-breach-offline` (the range service always fails) so `PasswordHealthUITests` needs no network.

## Sync

`Shared/Sources/Shared/Sync` mirrors the local vault to a list of remotes (ADR-0043): Google Drive and any number of
folders picked in the Files picker (iCloud Drive). `RemoteList` keeps them in `device/remotes.json` (a folder is a
security-scoped bookmark); each remote has its own sync record (`sync-state.json` is Drive's, `sync-state-<id>.json` a
folder's). `RemoteSyncCoordinator` runs every remote on its own with its own back-off, same-vault check and
remote-lost confirmation, so one failing never blocks another. `CoordinatedFolderStore` is the folder as a
`BlobStoreFfi` through `NSFileCoordinator`: an evicted file is `NotYetAvailable` after a download request, never
missing; the root folder is never created. `FolderRemote` holds the real-path check (a folder may not be, contain or
sit inside the local vault, checked on every open), the iCloud-off check and the rule that an existing vault goes only
to an empty folder (any other file, even one with a non-store name, makes it non-empty) or one holding it. The app side is `App/Sync`: `SyncModel` (foreground loop, per-remote rows),
`BackgroundSync` (BGAppRefresh, `com.figlock.ios.sync`) and the wording of each state. The status line under the
Vault title follows `SyncStatusLine` in Shared (Syncing, Synced for 3 s after a sync that ran, Offline, or a tappable
problem title that opens Sync settings; nothing otherwise).

Debug builds accept `-figlock-demo-remotes` (with `-figlock-seed-demo -figlock-unlock-demo`) to add one working and
one missing folder remote, so the sync status line and the Sync screen can be looked at in a simulator. A simulator build
needs ad hoc signing to get its App Group: add `CODE_SIGN_IDENTITY=- CODE_SIGNING_ALLOWED=YES` to the xcodebuild call.

## Build and run

Needs Xcode with an iOS simulator runtime and the tools pinned in `mise.toml` (`just setup`; it also installs the
two Rust iOS targets and XcodeGen).

```sh
just ios-xcframework   # cross-compile figlock-ffi (device + Apple silicon simulator), generate bindings
just ios-project       # generate FigLock.xcodeproj
just ios-test          # Shared package tests on a simulator (the extension memory test runs alone in a second run)
just ios-uitest        # UI tests (FigLockUITests) on a simulator against the demo vault
just ios-lint          # SwiftLint and swift-format (strict); `just ios-fmt` fixes formatting
just ios-run           # build, then install and launch on the "iPhone 17" simulator (SIMULATOR=<name or id> overrides)
just ios-run-ipad      # the same on the "iPad Pro 11-inch" simulator
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
workflow). It runs `just ios-lint`, `just ios-test`, `just ios-build` and `just ios-uitest`. The required check in the main ruleset is
the aggregate `ci` job, which counts a skipped `ios` job as passing, so no ruleset change is needed.
