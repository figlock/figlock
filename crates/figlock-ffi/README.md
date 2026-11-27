# figlock-ffi

UniFFI exports of `figlock-core` for the iOS (Swift) and Android (Kotlin) apps. The bindings are generated
from the compiled library at build time and are not committed, so they always match the core they were built
with.

## Generating the bindings

```sh
cargo build -p figlock-ffi --features bindgen
cargo run -p figlock-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library target/debug/libfiglock_ffi.dylib --language swift --out-dir out/swift
cargo run -p figlock-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library target/debug/libfiglock_ffi.dylib --language kotlin --out-dir out/kotlin
```

Use `.so` instead of `.dylib` on Linux and Android builds. App builds cross-compile the library per target
and run the generator against it the same way. The `bindgen` feature stays off for app builds.

## What crosses the boundary

- Keys and decrypted items stay behind opaque handles (`VaultSession`, `CreatedVault`, `DeviceKeysHandle`).
- Listings return non-secret fields only; a secret value comes out through `reveal_field` alone.
- `FigLockError` has three cases and no text from the input, so an error cannot leak a secret.
- No I/O and no clock in the vault calls: the app reads and writes `vault.json`, `devices/` and `log/` itself,
  passes bytes in, and supplies `now_ms`.
- Strings and byte arrays are copied by the binding layer. The copy on the Swift or Kotlin side is the app's
  to keep short-lived and overwrite.

## Joining a vault from the phone

`invite_path(code, vault_json)` says where to read the `.inv`; `start_join(code, inv, device, name, now_ms)`
returns the request file to store and a `JoinHandle`. Show `handle.fingerprint()` (the trusted device shows
the same one), poll for `response_path()` and `record_path()`, then `handle.finish(...)` with the typed
password gives the unlocked session and the Secret Key text for the Keychain. A wrong code, a bad or expired
invite, a tampered answer and a wrong password all fail with the same `Failed`. The trusted side (creating an
invite, approving) is not exported yet; it comes with the iOS approve flow.

## Quick unlock

`derive_unlock_key` gives an opaque `UnlockKey` (the Account Unlock Key). `UnlockKey.seal_for_quick_unlock(qk_public,
vault_id, device_id)` wraps it to the Secure Enclave public point (ADR-0025); `quick_unlock_ephemeral_public(blob)`
tells the chip which point to agree with, and `open_quick_unlock(blob, qk_public, shared, vault_id, device_id)` gives
the key back from the 32 byte ECDH result. `unlock_vault_with_key` opens the vault with it. The key never crosses
the boundary as bytes; the ECDH result is wiped after use.

## Sync

Sync is the one Rust engine of `figlock-sync` (ADR-0041 decision 3), with the parts that need platform APIs
passed in as UniFFI foreign traits:

- `BlobStoreFfi`: the five storage operations of `sync.md` 2. Swift implements it over a folder with
  `NSFileCoordinator` (the local container and the iCloud Drive folder). `FolderStore` is a plain Rust folder
  for stores that need no coordination, and `DriveStore` is the Google Drive adapter, which asks the app's
  `AccessTokens` for a token (sign-in with PKCE and the refresh token stay in Swift).
- `SyncClock` and `SyncStateStore`: the time, and the small record of whether this device has synced before.
- `SyncEngine(local, remote, state, clock, ownDevice)` binds them. `run(confirmReupload)`, `fillLocal()`,
  `openFromRemote()`, `exchange(first)`, `removeRemote(paths)` and `deleteRemote(typed)` are the same
  operations the CLI and desktop use. A second call while one runs fails with `Busy`.
- `SyncFailure` has no text, so a store error, path or token cannot end up in a log line. A foreign store
  returns `StoreFailure` cases (`NotFound`, `Exists`, `NotYetAvailable`, `Unauthorized`, ...); anything else
  it throws becomes `Other`.

The engine copies a few files at once, so the app's store is called from several threads at the same time:
make every method safe for concurrent calls on different paths. `tests/sync.rs` plays the app's parts in
Rust and `tests/swift/main.swift` supplies them from Swift, covering a full round, a lost remote, another
vault, a damaged and a tampered `vault.json`, a revocation, refused sign-in and the typed deletion.

## Zeroizing and threads

- Rust zeroizes every password, Secret Key and secret field value it receives as soon as the call is done,
  and everything inside a handle when it is locked or dropped. Call `lock()` when the app goes to the
  background; it clears keys and plaintext even if another call is in flight.
- The Swift and Kotlin copies are not Rust's to wipe. Keep secret `String` and `Data` values short-lived,
  overwrite `Data` you own after use, and never log them.
- Every handle is `Send + Sync`: calls from several threads take turns, so call them from a background task
  (Argon2id in `unlock_vault` and `create_vault` takes about a second on a phone) rather than the main
  thread. `tests/swift/main.swift` hammers one session from 64 threads and locks it mid-flight.

## Tests

`tests/api.rs` calls the exported API directly, with the negative cases (wrong password or Secret Key,
tampered files, locked use, bad ids). `tests/bindings.rs` runs the generator for both languages and, when
`swiftc` is installed, compiles and runs `tests/swift/main.swift` against the generated Swift and the real
library. `tests/sync.rs` covers sync through the foreign traits. `tests/join.rs` covers pairing from the new device with the trusted side played by `figlock-core`.

The Swift program also runs the published vectors of `docs/spec/vectors/` for every rule the API can decide
alone (Secret Key text, invite codes, fingerprints, KDF limits). Derivations that need fixed nonces run
against the Rust core, which has the vector-only hooks that must never ship in an app.
