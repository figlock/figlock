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
- No I/O and no clock: the app reads and writes `vault.json`, `devices/` and `log/` itself, passes bytes in,
  and supplies `now_ms`.
- Strings and byte arrays are copied by the binding layer. The copy on the Swift or Kotlin side is the app's
  to keep short-lived and overwrite.

## Joining a vault from the phone

`invite_path(code, vault_json)` says where to read the `.inv`; `start_join(code, inv, device, name, now_ms)`
returns the request file to store and a `JoinHandle`. Show `handle.fingerprint()` (the trusted device shows
the same one), poll for `response_path()` and `record_path()`, then `handle.finish(...)` with the typed master
password gives the unlocked session and the Secret Key text for the Keychain. A wrong code, a bad or expired
invite, a tampered answer and a wrong password all fail with the same `Failed`. The trusted side (creating an
invite, approving) is not exported yet; it comes with the iOS approve flow.

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
library. `tests/join.rs` covers pairing from the new device with the trusted side played by `figlock-core`.

The Swift program also runs the published vectors of `docs/spec/vectors/` for every rule the API can decide
alone (Secret Key text, invite codes, fingerprints, KDF limits). Derivations that need fixed nonces run
against the Rust core, which has the vector-only hooks that must never ship in an app.
