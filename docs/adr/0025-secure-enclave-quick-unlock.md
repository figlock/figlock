# ADR-0025: Hardware-bound quick unlock with the Secure Enclave (CLI on macOS)

- Status: Proposed
- Date: 2026-11-09
- Decision: D25 in `docs/decisions.md`

## Context

`crypto.md` 8 says quick unlock wraps the Account Unlock Key (`AUK`) under a hardware-bound key `QK` and that
a biometric check releases it. Today the CLI keeps `AUK` as a login-keychain item behind a Touch ID prompt
(FIG-69), so the check is enforced by the app and the keychain ACL, not by hardware: anything that can read
that keychain item as the user, or patches the app, skips the prompt. FIG-70 asks for the real thing.

A Secure Enclave key cannot be kept in the keychain by a bare command-line binary: creating it with
`kSecAttrIsPermanent` fails with -34018 (missing restricted entitlement, which needs an app bundle and a
provisioning profile). Measured on an Apple M2 (macOS 26): a CryptoKit `SecureEnclave.P256.KeyAgreement`
key can be created and reloaded from its `dataRepresentation` by a bare binary with no entitlements. That blob
is opaque and only the same chip can use it.

## Decision

- **`QK` is a P-256 key agreement key in the Secure Enclave**, created with `.privateKeyUsage` and
  `.userPresence` (Touch ID, or the device password where there is no biometric) and
  `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`. Its `dataRepresentation` is stored by FigLock in
  `quick-unlock.qk` in the FigLock home (`0600`, never synced). The blob is useless without this chip and a
  successful user-presence check, so it is not a secret in the file-on-disk sense.
- **The wrap of `AUK` is an ECIES-style construction from standard primitives** (the Secure Enclave only offers
  P-256 ECDH, a platform API, which `AGENTS.md` allows):

  ```
  eph        = fresh software P-256 key pair
  Z          = ECDH(eph_secret, QK_public)                      32 bytes, the x coordinate
  K          = HKDF-SHA256(ikm = Z, salt = eph_public || QK_public,
                           info = "figlock quick-unlock se v1" || vault_id || device_id, len = 32)
  qu_blob    = 0x01 || eph_public (65 bytes, uncompressed) || seal(K, AUK, aad("quick-unlock", vault_id, device_id))
  ```

  `seal` is the XChaCha20-Poly1305 format of `crypto.md` 3. Opening runs `ECDH(QK_secret, eph_public)` inside
  the Secure Enclave, which asks for user presence, then the same HKDF and `open`. Encrypting needs no
  prompt, decrypting does, every time.
- **The software half has test vectors** (fixed `Z`, ephemeral public key, ids, nonce, `AUK` to the blob and
  back, plus negative cases: wrong ids, flipped bit, wrong version byte, truncated blob). The Secure Enclave
  half cannot be vectored; it is tested on a Mac by a test that is skipped where there is no Secure Enclave.
- **A Swift shim exposes it to Rust.** One Swift file with a C interface (create, agree, available), compiled by
  `build.rs` of `figlock-biometric` on macOS only with `swiftc`, once per target arch so the universal
  binary still builds. The Swift runtime is part of macOS, so users install nothing extra. If `swiftc` is
  missing at build time, or there is no Secure Enclave at run time, the CLI keeps the current keychain item,
  and `quick-unlock status` says which kind of protection a device has.
- **Migration.** A device with a keychain `AUK` item is moved on its next successful quick unlock: create `QK`,
  wrap the `AUK` it just released, store the blob, delete the keychain item. Failures leave the keychain item
  in place. `quick-unlock disable`, a password change and the policy limits in `crypto.md` 8 delete the blob
  and the key the same way they delete the keychain item today.
- **`.userPresence` does not invalidate when a fingerprint is enrolled or removed.** `biometryCurrentSet` would,
  but it would also lock out anyone using the device password. Keeping `.userPresence` matches today's
  behaviour; a stricter option can be added later.

## Consequences

- A thief with the user's files and a logged-in session still cannot use `QK` without Touch ID or the device
  password, and cannot move the blob to another Mac.
- A new `p256` crate (RustCrypto) is needed for the software half; `cargo-deny` already covers its licence.
- The build gains a Swift step on macOS. Linux and other targets are untouched. Release builds use the
  existing macOS runner and the signed binary is what users get.
- Tests that need a real Touch ID prompt cannot run in CI; the unit and vector tests can, and the prompt is
  checked by hand on the signed binary before a release.
- The format of `vault.json` and the op log is unchanged.

## Alternatives considered

- **Keep the keychain item.** No hardware enforcement; that is today's behaviour and the thing FIG-70 exists to
  improve.
- **Persist a `SecKey` in the keychain.** Fails with -34018 for a bare binary.
- **Ship the CLI inside a signed `.app` with a provisioning profile.** Makes the keychain route work but
  changes how people install the CLI; better done with the desktop app (FIG-43).
- **Encrypt with the Secure Enclave key directly (`SecKeyCreateEncryptedData`).** Needs the `SecKey`, which
  cannot be reloaded without the keychain; CryptoKit's ECDH plus our own wrap avoids that.
