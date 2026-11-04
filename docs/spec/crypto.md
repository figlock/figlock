# FigLock crypto specification

Status: draft v1 (FIG-16). Covers key derivation, the key hierarchy, encryption, signing, quick unlock,
rotation and versioning. The sync format is in `sync.md` (FIG-17) and pairing and recovery in
`pairing-recovery.md` (FIG-18). Test vectors for every rule here live in `docs/spec/vectors/` (FIG-20); a rule
without a vector is not done.

The words MUST, SHOULD and MAY are used as in RFC 2119.

## 1. Goals and non-goals

Goals:

- Storage (Google Drive, iCloud Drive, a folder) sees only ciphertext, counts, sizes and timestamps.
- A stolen copy of the vault cannot be brute-forced from a weak master password, because a 128-bit Secret
  Key is also required.
- Every device can work offline with a full local copy.
- A revoked device cannot read anything written after revocation.
- Only standard primitives, one construction per purpose, no custom cryptography.

Non-goals:

- Protecting against malware on an unlocked device, or a fully malicious agent with shell access.
- Hiding file counts, sizes or timestamps from the storage provider.
- Post-quantum security. Versioning (section 12) leaves room to add it.

## 2. Primitives

| Purpose                  | Primitive                              | Rust crate (RustCrypto) |
| ------------------------ | -------------------------------------- | ----------------------- |
| Password hashing         | Argon2id v1.3, RFC 9106                | `argon2`                |
| Key derivation           | HKDF-SHA-256, RFC 5869                 | `hkdf`, `sha2`          |
| Authenticated encryption | XChaCha20-Poly1305 (24-byte nonce)     | `chacha20poly1305`      |
| Signatures               | Ed25519 (pure), RFC 8032               | `ed25519-dalek`         |
| Key agreement            | X25519, RFC 7748                       | `x25519-dalek`          |
| Hash                     | SHA-256                                | `sha2`                  |
| Randomness               | OS CSPRNG (`getrandom`; `crypto.getRandomValues` in WASM) | `getrandom` |
| Zeroization              | `zeroize` on every secret buffer       | `zeroize`               |

No other primitive may be used without an ADR and new test vectors.

## 3. Conventions

- All keys are 32 bytes unless stated. Integers in framing are big-endian.
- `||` is byte concatenation.
- Every `HKDF` call uses SHA-256 and an explicit `info` string of the form `figlock/v1/<purpose>`. Distinct
  purposes MUST use distinct `info` strings. Unless stated, the HKDF salt is empty (a string of 32 zero
  bytes per RFC 5869).
- `lp(x)` is `u32(len(x)) || x`.
- **AAD.** `aad(ctx, parts...) = u16(len(ctx)) || ctx || lp(part1) || lp(part2) || ...` where `ctx` is an ASCII
  string naming the use. AAD binds every ciphertext to what it is for, so a ciphertext cannot be replayed in
  another place.
- **Envelope.** Every AEAD ciphertext is stored as `0x01 || nonce(24) || ciphertext || tag(16)`. The first byte
  is the format version (section 12). Nonces are 24 fresh random bytes per encryption, never reused or
  derived. With XChaCha20 random nonces are safe for any realistic number of messages per key.
- `seal_to(pub, plaintext, aad)` encrypts to an X25519 public key with an ephemeral key pair, as defined in
  `pairing-recovery.md` 7.1; it is the only public-key encryption construction in FigLock.
- `seal(key, plaintext, aad)` and `open(key, envelope, aad)` denote the above. `open` MUST fail on any
  mismatch and MUST NOT return partial plaintext.
- **Signed JSON** files (`vault.json`, `devices/*.json`) are canonicalised with JCS (RFC 8785) before signing.
  Op and snapshot payloads inside ciphertext are deterministic CBOR (RFC 8949 section 4.2).
- **Text.** Master passwords are normalised to Unicode NFKD, encoded as UTF-8, with no trimming. The same
  rule applies on every platform.
- **Identifiers.** `vault_id`, `device_id` and `item_id` are 16 random bytes, shown as lowercase hex.

## 4. Secrets and where they live

| Name | Size | Created | Stored |
| ---- | ---- | ------- | ------ |
| Master password `pw` | user chosen | by the user | nowhere; in the user's head or password manager |
| Secret Key `SK` | 128 bit | at vault creation | Emergency Kit (paper or file), and on a device only wrapped by quick unlock |
| Account Unlock Key `AUK` | 256 bit | derived at each full unlock | memory only (and hardware-wrapped for quick unlock, section 8) |
| Vault Key `VK` | 256 bit | at vault creation, random | storage, wrapped by `AUK` (and by the Recovery Key if enabled) |
| Item key `IK` | 256 bit | per item, random | inside the vault, wrapped by `VK` |
| Device signing key | Ed25519 | per device | device keystore only; never leaves the device |
| Device agreement key | X25519 | per device | device keystore only |
| Recovery Key `RK` (optional) | X25519 pair | on request | `rk_pub` in `vault.json`; `rk_priv` printed or split with SLIP-0039, never stored (`pairing-recovery.md` 7) |

### 4.1 Secret Key format

`SK` is 16 random bytes. For display it is extended with a 4-byte check and written as Crockford base32
(32 characters, upper case, no padding):

```
check = SHA-256("figlock/v1/sk-check" || SK)[0..4]
text  = "FL1-" + groups of 4 characters of base32(SK || check), joined by "-"
        e.g. FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB
```

Parsers accept lower case, ignore dashes and spaces, and map `I`, `L` to `1` and `O` to `0` (Crockford).
A wrong check MUST be reported as "typo in Secret Key", not as a wrong password.

## 5. Key derivation

Inputs: `pw`, `SK`, `vault_id`, `salt` (16 random bytes, public), Argon2 parameters `(m, t, p)` (public).

```
pw_key = Argon2id(password = NFKD(pw), salt = salt, m, t, p, output = 32 bytes)
sk_key = HKDF(ikm = SK, salt = vault_id, info = "figlock/v1/secret-key", L = 32)
AUK    = HKDF(ikm = pw_key || sk_key, salt = vault_id, info = "figlock/v1/account-unlock-key", L = 32)
```

Argon2id has no secret and no associated data inputs (both empty). Both inputs are required: with a
stolen vault and no Secret Key an attacker faces a 128-bit search no matter how weak `pw` is.

### 5.1 Argon2 parameters

Default profile at vault creation: `m = 65536 KiB (64 MiB), t = 3, p = 4` (RFC 9106 second recommended
option). Parameters are stored in `vault.json`, and one parameter set applies to the whole vault so that
every device derives the same key.

- Creation picks the default, or a stronger set if a benchmark on the creating device shows the unlock time
  stays under 1 s. It never picks weaker than the floor.
- Floor (a client MUST refuse to create or accept below it): `m >= 32768 KiB, t >= 2, p >= 1`.
- Ceiling (a client MUST refuse above it, to stop a hostile `vault.json` exhausting memory):
  `m <= 1048576 KiB (1 GiB), t <= 10, p <= 16`.
- A password change MAY raise the parameters. Lowering below the default requires an explicit user choice
  and never goes below the floor.
- Devices that cannot allocate `m` (for example a low-memory browser tab) show an error; they do not
  downgrade the vault.

A storage attacker who edits the parameters in `vault.json` cannot gain anything: `AUK` depends on them, so
the wrong values make `open` of the wrapped `VK` fail, and they are also in its AAD.

## 6. Key hierarchy

```
 pw ──Argon2id──┐
                ├─HKDF─► AUK ──unwraps──► VK ──HKDF──┬─► LK   op/snapshot encryption key
 SK ──HKDF──────┘                         │          ├─► WK   item key wrapping key
                                          │          └─► authority seed ► authority key (Ed25519)
                                          └──wraps──► IK per item (via WK)

 RK (optional): rk_priv ──opens seal_to(rk_pub)──► VK   (separate wrap, pairing-recovery.md 7)
 device keys: Ed25519 signing key + X25519 agreement key, one pair per device
```

Derivations from `VK` (HKDF-SHA-256, empty salt):

```
LK             = HKDF(VK, info = "figlock/v1/log-key",         L = 32)
WK             = HKDF(VK, info = "figlock/v1/item-key-wrap",   L = 32)
authority_seed = HKDF(VK, info = "figlock/v1/authority-seed",  L = 32)   # Ed25519 private seed
```

The **authority key** is the Ed25519 key pair generated from `authority_seed`. It is the vault's trust
anchor: it signs `vault.json` and the device records. Any device holding `VK` can derive it, so there is no
separate secret to store, and a device that verifies a signature derives the public key itself rather than
trusting a value read from storage.

### 6.1 Vault Key wrap

```
wrapped_vk = seal(AUK, VK,
                  aad("vk-wrap", vault_id, u32(key_epoch), kdf_params_bytes))
```

`kdf_params_bytes` is `u32(m) || u32(t) || u32(p) || lp(salt)`. It is stored in `vault.json` as
`wrapped_vk`.

## 7. Encrypting vault data

### 7.1 Items

An item is a set of named fields (title, type, URLs, username, secret fields, tags, notes, custom fields).
Its own key `IK` is 32 random bytes. Each field value is sealed on its own so that devices can merge edits
field by field (`sync.md`) without ever decrypting more than they need:

```
wrapped_ik  = seal(WK, IK, aad("item-key", vault_id, item_id, u32(key_epoch)))
field_blob  = seal(IK, cbor(value), aad("field", vault_id, item_id, field_id, hlc_bytes))
```

`hlc_bytes` is the 8-byte hybrid logical clock stamp of the write (`sync.md` section 5), so a field value
cannot be replayed under another stamp, field or item. Titles, URLs and usernames are fields, so they are
encrypted like everything else. Per-item keys let one item be re-keyed, exported or (later) shared without
exposing the vault.

### 7.2 Op batches, snapshots and the audit log

Files that devices write to storage (formats in `sync.md`) are encrypted as a whole with the log key:

```
envelope = seal(LK, cbor(payload),
                aad("log", vault_id, device_id, u64(seq), u32(key_epoch)))
```

`seq` is the file's sequence number in that device's folder. Because the AAD includes `device_id` and `seq`,
a file moved to another name or device fails to open. Snapshots use `aad("snap", ...)` with the same parameters. Item data inside the payload is already sealed under
`IK` (7.1), so a leaked `LK` alone does not reveal item contents.

### 7.3 Signing

Every op and snapshot file is signed by the device that wrote it. Sign the envelope, not the plaintext, so
a signature is checked before any decryption.

```
signature = Ed25519.sign(device_signing_key,
              "figlock/v1/file-sig" || 0x00 || vault_id || device_id || u64(seq) || SHA-256(header || envelope))
```

`header` is the plaintext file header defined in `sync.md` section 4 (magic, version, kind, `key_epoch`), so
the epoch that selects the decryption key is covered by the signature as well as by the AAD.

A reader MUST verify, in order: (1) `device_id` has a valid, non-revoked device record, (2) the signature,
(3) then decrypt. A file failing any step is ignored and reported, never partially applied.

### 7.4 Device records

`devices/<device_id>.json` holds `device_id`, name, `sign_pub` (Ed25519), `agree_pub` (X25519), `added_at`
and `epoch_added`. The authority key signs the JCS form of the record:

```
sig = Ed25519.sign(authority_key, "figlock/v1/device-record" || 0x00 || JCS(record))
```

A record without a valid authority signature is not a device. `vault.json` is signed the same way with
context `figlock/v1/vault-json`, so that after unlock any tampering with `salt`, parameters, epoch or
`min_format_version` is detected.

## 8. Quick unlock (biometrics)

Quick unlock is local to a device and is never synced.

- The device creates a hardware-bound key `QK` that requires user presence: Secure Enclave (macOS, iOS),
  Android Keystore with `setUserAuthenticationRequired`, or TPM / Windows Hello. `QK` is non-exportable
  where the platform allows.
- After a full unlock the device stores `qu_blob = seal(QK, AUK, aad("quick-unlock", vault_id, device_id))`
  (or the platform's equivalent wrap). A biometric check releases `QK` use and yields `AUK`.
- The CLI broker daemon, desktop and mobile apps use the same blob format where the platform permits;
  where the platform gives only a keychain item protected by biometrics, that item holds `AUK` directly.
- A full master-password unlock is required: after a reboot or app restart, after 7 days since the last full
  unlock, after 5 failed biometric attempts, after a password change or key rotation, and when the vault is
  locked by the user. These are defaults; users MAY tighten them.
- Disabling quick unlock or revoking the device deletes `qu_blob` and `QK`.

The web app (PWA) MAY keep `qu_blob` under a non-extractable WebCrypto key, and MAY offer passkey PRF as an
additional unlock path. PRF is never the only root of trust (research: PRF support and loss risks).

## 9. Changing keys

| Event | What changes | Data re-encrypted? |
| ----- | ------------ | ------------------ |
| Change master password | New `pw`, new `salt`, new `AUK`; rewrap `VK`; update `vault.json` | No |
| Change Argon2 parameters | Same as above | No |
| Rotate Secret Key | New `SK`, new `AUK`; rewrap `VK`; new Emergency Kit | No |
| Revoke a device | New `VK` (epoch + 1), see 9.1 | Item keys rewrapped; payload files rewritten by compaction |
| Suspected `VK` compromise | Same as revoke | Same |

After any change the old wrapped material is removed from `vault.json`, and other devices learn of it at
their next sync and ask for a full unlock.

### 9.1 Vault Key rotation

A revoked device still knows the old `VK`, so revocation MUST rotate it:

1. Generate `VK'`, `key_epoch' = key_epoch + 1`. Derive `LK'`, `WK'`, `authority'`.
2. Rewrap every `IK` under `WK'` and rewrite device records and `vault.json` with the new authority key,
   leaving the revoked device out and recording it in a signed `revoked` list.
3. Write a new snapshot (sync spec) sealed under `LK'`. Old op files stay readable by devices that hold the
   previous epoch key, which is kept in `vault.json` as `epoch_keys`: `seal(VK', VK_old, aad("epoch-key", ...))`.
4. Rewrap `VK'` under `AUK`, and for `rk_pub` if a Recovery Key is enabled (this needs only the public key).

Rotation cannot recall data a revoked device already copied; it only protects what is written afterwards.
This limit MUST be stated in the UI and the threat model (FIG-19).

## 10. Memory and implementation rules

- Secret values (`pw`, `SK`, `pw_key`, `AUK`, `VK`, `LK`, `WK`, `IK`, device private keys, plaintexts) are held
  in types that zeroize on drop, are never `Clone` by accident, never implement `Debug`/`Display`, and never
  appear in logs, errors or panic messages.
- Compare secrets and MACs in constant time. AEAD tag checking is done by the library.
- Lock memory pages (`mlock` or the platform equivalent) for long-lived keys where available, and disable
  core dumps in the broker daemon.
- Randomness only from the OS CSPRNG. A failure to obtain randomness aborts; there is no fallback.
- Decryption failures return one generic error to callers. Detailed reasons go only to a local debug log
  that never includes key or plaintext bytes.
- Unknown `info`, `ctx` or version values are errors, never ignored.

## 11. What the storage provider and a thief can learn

| Attacker | Learns | Cannot |
| -------- | ------ | ------ |
| Storage provider | file counts, sizes, timestamps, `vault_id`, KDF parameters, device ids, number of devices | titles, URLs, usernames, secrets, item counts per type |
| Thief with a copy of the vault, no `SK` | the above | brute-force `pw` (128-bit `SK` needed) |
| Thief with the vault and `SK`, no `pw` | an offline guessing target costing `m`/`t`/`p` per guess | skip the KDF |
| Revoked device | everything up to its revocation | anything written in the new epoch |
| Active storage attacker (write access) | can delete or withhold files, roll back to older files | forge ops (signatures), alter parameters undetected |

Rollback and withholding by a storage attacker are handled by the sync spec (sequence gaps, per-device
counters, snapshots signed with a high-water mark).

## 12. Versioning and agility

- Every envelope starts with a format version byte (`0x01` for this spec). A reader that sees a higher
  version than it supports MUST stop and tell the user to update, and MUST NOT write.
- `vault.json` carries `min_format_version` and `suite` (`"fl1"`: the primitives in section 2). Raising
  `min_format_version` is signed, so an attacker cannot lower it to force an old suite.
- A new suite (for example a post-quantum KEM) gets a new suite id and its own `info` strings. Suites are
  introduced by ADR with test vectors, and migration rewrites the vault, never mixes suites in one file.

## 13. Required test vectors (for FIG-20)

Vectors are JSON with inputs and expected outputs in hex, produced by the reference implementation and
reproduced by a second, independent implementation (for example Python `argon2-cffi` and PyNaCl) before
merge. Each rule gets a positive and a negative case.

1. Secret Key encode / decode, check mismatch, lower-case and confusable characters.
2. Argon2id with the default and floor parameters; NFKD normalisation of a non-ASCII password.
3. `sk_key`, `AUK` derivation end to end.
4. `VK` wrap/unwrap; wrong `AUK`, wrong parameters in AAD, wrong epoch.
5. `LK`, `WK`, `authority_seed` and the authority public key.
6. Item seal/open; wrong `item_id`, wrong version, tampered tag.
7. Log envelope seal/open; wrong `device_id` or `seq`.
8. File signature; wrong device, flipped envelope bit, revoked device.
9. Device record and `vault.json` signatures; altered parameter detected.
10. Vault Key rotation: epoch keys chain, old-epoch file still opens with `epoch_keys`.
11. Quick-unlock blob format (software-key variant only; hardware wraps are tested on devices).

## 14. Open questions

- Argon2 default `p = 4`: WASM runs lanes sequentially without threads, so unlock may be slower in the PWA.
  Benchmark in FIG-24; the floor and default can move by ADR before the first release.
- Whether the audit log (agent accesses) gets its own key so the broker can append without `VK` (phase 2).
