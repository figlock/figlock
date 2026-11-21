# ADR-0041: iOS app architecture - app group vault, Rust sync engine, shared keychain, iPhone only

- Status: Accepted
- Date: 2026-11-22
- Decision: D41 in `docs/decisions.md`
- Builds on: ADR-0004 (native shell, shared Rust core), ADR-0025, ADR-0026, ADR-0027, ADR-0034, ADR-0037, ADR-0038
- Task: FIG-153. Unblocks FIG-154 (FFI), FIG-155 (build), FIG-159 (storage), FIG-160 (unlock), FIG-166 (AutoFill)

## Context

ADR-0004 fixes the stack (SwiftUI over `figlock-core` through UniFFI) but says nothing about how the iPhone
app stores a vault, syncs it, keeps keys, or runs the AutoFill extension. Every iOS ticket depends on those
answers, and several are expensive to change later. What the code and specs say today:

- `figlock-ffi` exports the pure core only. It has **no I/O and no clock** (its README): the app reads and writes
  `vault.json`, `devices/` and `log/` itself and passes bytes in.
- Sync (ADR-0026) lives in `figlock-session::sync` and runs against a `Config` and `Place`, with desktop-only
  parts around it (`security-framework`, `figlock-biometric`, a loopback OAuth listener). It cannot be linked
  into an iOS target as it is.
- The `BlobStore` contract (`sync.md` 2) is five operations, and the folder and Drive adapters already pass the
  same contract tests. The sync algorithm itself (mirror, back-off, remote lost, same-vault check) is the part
  that is easy to get subtly wrong twice.
- A credential provider extension is a separate process with a small memory limit and no long-running
  background time. It cannot run an Argon2id derivation (`crypto.md` uses memory-hard parameters), and it
  cannot be allowed to make decisions about sync or keys on its own.
- On iOS a Secure Enclave key can be a permanent keychain key shared through a keychain access group, which a
  bare command-line binary cannot do (ADR-0025). The app and extension are bundle signed, so they can.
- iCloud Drive files can be listed but not yet downloaded ("not available yet", `sync.md` 2), and a folder the
  person picked outside our container is reached through a security-scoped bookmark that is awkward to share
  with an extension.
- The owner uses an iPhone. iPad has no ticket in v1 (FIG-178).

## Decision

**1. Targets and platform.** One iOS app target (SwiftUI) and one AutoFill Credential Provider extension target,
both using a shared Swift package (`Shared`) that wraps the generated bindings. Bundle ids, the App Group and
the keychain access group are created under the existing Apple Developer account (FIG-44). The app is built
for **iPhone only** in v1 (`TARGETED_DEVICE_FAMILY = 1`, not offered as an iPhone app on Mac or Vision). Minimum
version is **iOS 18** (confirmed by the owner: modern credential provider API, one test matrix). iPad is a later task (FIG-178) with its own layout ADR; nothing in v1 may assume a phone
screen size in the core or in `Shared`.

**2. The vault is always a local folder in the App Group container** (the ADR-0026 rule, with the container as
the default folder). The app writes to it; the extension only reads it. Writes are atomic (temporary file and
rename) and every read tolerates a concurrent write, so no cross-process lock is needed. The container is
excluded from device backups (the vault is ciphertext, but the quick unlock blob and device identity must not
move to another phone); file protection class is `completeUntilFirstUserAuthentication` so the extension can read
while the phone is locked-but-unlocked-once, and the vault stays ciphertext at rest regardless.

**3. Sync runs the one Rust engine, with storage passed in.** The sync algorithm moves out of
`figlock-session` into a platform-neutral crate (working name `figlock-sync`, the same extraction pattern as
`figlock-broker`, ADR-0040). It works against `BlobStore` and a small `Clock` and `SyncState` trait and holds no
paths or globals. `figlock-ffi` exports it with **foreign traits** (UniFFI callback interfaces), so Swift
supplies the parts that need Apple APIs:
- `BlobStore` over a folder with `NSFileCoordinator` (the local container and the iCloud folder remote);
- the OAuth access token for Google Drive (sign-in runs in Swift with `ASWebAuthenticationSession` and PKCE
  against an iOS OAuth client, ADR-0021; the refresh token is kept in the keychain);
- the clock and the small sync state store.
The Drive `BlobStore` stays the Rust adapter in `figlock-store` (`gdrive` feature, `ureq`), fed the token by
Swift. The alternative of rewriting sync in Swift is rejected: it doubles the audit scope that ADR-0004 exists
to avoid. FIG-154 splits the work: the extraction and the foreign-trait exports, with the existing sync tests and
the CLI and desktop behaviour unchanged.

**4. Remote is one of `drive` or `folder`.** ADR-0026 has an optional remote `drive`. On iPhone the same slot also
accepts `folder`: a folder the person picks in the Files picker (typically `iCloud Drive/FigLock`, the same
folder the Mac uses, ADR-0002), mirrored from and to the local container exactly like Drive. The picked folder is
never the working vault, so the extension and the app always read one stable local copy, and a not-yet-downloaded
iCloud file is "retry", never "missing". Only one remote is active at a time, as on desktop. The desktop
config and `config.json` do not change; iOS stores its own device config in the container.

**5. Sync timing.** The app syncs on launch, on foreground, after a local commit, on pull-to-refresh and on a best
effort background refresh task. The extension never syncs and never writes. iOS gives no guarantee of
background time, so a stale copy shows its "last synced" time (ADR-0026) rather than hiding the problem.

**6. Keys.**
- `QK` is a **Secure Enclave P-256 key** made permanent in the keychain, in the **shared keychain access group**,
  with `.userPresence` (Face ID, falling back to the device passcode) and `ThisDeviceOnly`. The `qu_blob` wrap of
  `AUK` is exactly the ADR-0025 construction (`crypto.md` 8), same test vectors, so the Swift side adds no new
  crypto. `qu_blob` lives in the container.
- The **Secret Key** is a shared-group keychain item, `ThisDeviceOnly`, **not** `kSecAttrSynchronizable` (it must
  never go to iCloud Keychain), read without a prompt (ADR-0037) so a master password unlock never shows Face ID.
  ADR-0034 (`sk_gen`) and ADR-0038 (opt-out and opt-in) apply unchanged.
- Device keys follow the same rule as the Secret Key.
- Full master-password unlock is required after the quick unlock limits of `crypto.md` 8. **"After an app restart"
  does not apply on iOS**, where the system ends and restarts processes at will; it is replaced by "after the phone
  restarts" (detected from the boot session) so the rule keeps its intent. This changes a default in `crypto.md`
  8 (confirmed by the owner); the same change is made to `crypto.md` 8 by FIG-160.
- Secret screens are blanked in the app switcher and screen capture is blocked (`DESIGN.md` 8). Clipboard copies use
  local-only, expiring pasteboard items.

**7. The AutoFill extension.**
- It **never runs a password derivation.** It unlocks only through `qu_blob` and `QK` (a Face ID prompt inside the
  extension), so the app must have been set up and unlocked with the master password at least once. If quick
  unlock is off or expired, it shows "Open FigLock to unlock" and hands over to the app.
- It opens the local copy read-only, applies the op logs to a **search-only view** (non-secret fields plus the
  one secret it was asked for) and releases it when done. Whether a full replay fits the memory limit with
  10,000 items is **measured in FIG-166 before the extension is built out**. If it does not fit, the app writes a
  compact autofill index sealed under `AUK` into the container after each commit, and the extension reads that
  instead. The index is a fallback only (confirmed by the owner): build it only if the measurement says the
  replay does not fit. Either way the extension holds one secret at a time and zeroizes it.
- Identity store: the app registers domain and username (no secrets) with `ASCredentialIdentityStore` so QuickType
  suggestions work; the store is cleared on lock-out of the vault and on "remove this vault from this iPhone".
- Passkeys and one-time codes are a spike (FIG-167), not part of the first extension.

**8. Onboarding.** A new iPhone joins by **pairing** from a trusted device (QR scan or typed code, `pairing-recovery.md`
3) as the main path, since that moves the Secret Key (ADR-0027). Creating a new vault on the phone, opening one
from Drive, and typing the Secret Key from the Emergency Kit stay available. The local-folder-missing recovery
screen of ADR-0026 is part of the app (FIG-161).

**9. Agent approvals.** An approval request reaches the phone through the relay and APNs (FIG-53). The phone shows the
approval card of `DESIGN.md` 8, asks for Face ID, and answers with an approval signed by the device key and bound
to the request id and nonce (threat model G9). No vault data goes through the relay or Apple's push service; the
push carries only an opaque id and the app fetches the encrypted request.

**10. Repo and build.** All iOS code is in this repo under `apps/ios/` (ADR-0015). The Xcode project is generated
from `project.yml` with XcodeGen and is not hand edited. `figlock-ffi` is cross-compiled to an xcframework
(`just ios-xcframework`); bindings are generated at build time and not committed. CI runs the iOS job only for
`apps/ios/**`, `crates/figlock-core/**`, `crates/figlock-ffi/**`, and the sync crate. Releases go to TestFlight
from a tag (FIG-175).

## Consequences

- One implementation of sync and merge on desktop and iOS, tested by the same suites. The cost is the extraction in
  FIG-154, which touches `figlock-session` and needs its tests green before the iOS work builds on it.
- The extension is deliberately dumb: read-only, no sync, no derivation. A phone that has never been unlocked by
  hand cannot autofill, which is the safe failure.
- The iCloud folder remote costs a second copy of the vault on the phone, in exchange for a reliable offline copy
  and a simple extension.
- `crypto.md` 8 gets an iOS note for "app restart", and `docs/spec/threat-model.md` gets a mobile section in
  FIG-171.
- iPad later reuses the same targets, with a layout ADR; the iPhone-only decision is a build setting and a design
  scope, not a code assumption.

## Alternatives considered

- **Reimplement sync in Swift:** smaller FFI, but two implementations of the part most likely to have subtle bugs,
  and the second one outside the property tests.
- **Make the picked iCloud folder the working vault:** matches the desktop folder model, but the extension then
  needs the bookmark and has to cope with undownloaded files on every fill.
- **Use CloudKit or an iCloud ubiquity container:** rejected in ADR-0002 (locks out Android and Windows).
- **Derive the key inside the extension with a lighter KDF:** a second, weaker unlock path; rejected.
- **Share the vault with the extension through a database such as SQLite:** a new store and format to audit; the op
  files already are the format.
- **Cross-platform UI (Flutter, React Native):** rejected in ADR-0004.
