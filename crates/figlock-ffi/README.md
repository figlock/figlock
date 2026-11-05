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

## Tests

`tests/api.rs` calls the exported API directly, with the negative cases (wrong password or Secret Key,
tampered files, locked use, bad ids). `tests/bindings.rs` runs the generator for both languages and, when
`swiftc` is installed, compiles and runs `tests/swift/main.swift` against the generated Swift and the real
library.
