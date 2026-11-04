# FigLock pairing and recovery specification

Status: draft v1 (FIG-18). Defines how a new device joins a vault, what the Emergency Kit contains, every
supported recovery path and the optional Recovery Key with Shamir split, and how a device is revoked.
Keys and primitives are in `crypto.md`; storage layout and sync in `sync.md`.

The words MUST, SHOULD and MAY are used as in RFC 2119.

## 1. Principles

- With no server, **nobody can reset a forgotten password for the user**, not us, not the storage provider.
  Recovery is therefore designed in, and the product says plainly which losses are recoverable.
- A device that holds the master password and the Secret Key is authorised by definition; pairing is a
  convenience that moves the Secret Key to a new device without typing it, not a gate that grants keys.
- Recovery paths are few, each is tested end to end, and each ends with the old credentials invalidated.

## 2. Terms

| Term | Meaning |
| ---- | ------- |
| Trusted device `T` | a device that is paired, unlocked and holds the vault key in memory |
| New device `N` | the device being added |
| Invite | a one-time, short-lived offer created on `T`, shown as a short code and QR |
| Emergency Kit | printable document holding the Secret Key and how to use it |
| Recovery Key `RK` | optional asymmetric key pair that can unwrap the Vault Key when the password is lost |

## 3. Device pairing

### 3.1 What `N` needs

To open the vault `N` needs three things: access to the storage (the same Google Drive account, folder or
sync service), the master password (typed by the user), and the Secret Key. Pairing delivers the third.
Alternatively the user types the Secret Key from the Emergency Kit and skips pairing (section 3.5).

### 3.2 Flow (online, with a trusted device)

```
T (unlocked)                         storage                         N (new)
1. user taps "Add a device"
   T makes invite secret s (100 bit),
   ephemeral X25519 pair (Et)
   writes inbox/<invite_id>.inv ───►
   shows code and QR (carries s)
                                                                  2. user chooses "Add this device",
                                                                     scans or types the code
                                                                     N derives invite_id from s,
                                                                     reads .inv, makes keys
                                                                     and ephemeral En
                                         ◄─── writes inbox/<invite_id>.req
3. T reads .req, derives K, opens it.
   T shows "Add 'Work MacBook'?" and
   the new device's key fingerprint
   user confirms with biometrics
   T writes devices/<N>.json (signed
   by the authority key) and
   inbox/<invite_id>.resp ────────►
   T deletes .inv, later .req/.resp
                                                                  4. N reads .resp, opens it, gets
                                                                     the Secret Key and device record.
                                                                     User types the master password.
                                                                     N derives AUK, unwraps VK,
                                                                     verifies the device record.
                                                                     N stores keys, deletes the files.
```

### 3.3 Cryptography of the invite

```
s          = 100 random bits, shown as 5 groups of 4 Crockford base32 characters: XXXX-XXXX-XXXX-XXXX-XXXX
invite_id  = HKDF(s, salt = vault_id, info = "figlock/v1/invite-id", L = 16)    # hex in the file name
```

1. `T` writes `inbox/<invite_id>.inv` (plaintext, public): `{ v, vault_id, invite_id, Et_pub, expires_at }`.
2. `N` derives `invite_id` from `s`, reads the `.inv`, generates its device keys and an ephemeral `En`, and
   computes
   ```
   shared = X25519(En_priv, Et_pub)
   K_req  = HKDF(ikm = shared || s, salt = invite_id, info = "figlock/v1/invite-req", L = 32)
   K_resp = HKDF(ikm = shared || s, salt = invite_id, info = "figlock/v1/invite-resp", L = 32)
   ```
3. `N` writes `inbox/<invite_id>.req` = `En_pub || seal(K_req, cbor({device_name, sign_pub, agree_pub}),
   aad("invite-req", vault_id, invite_id))`.
4. `T` reads it, derives the same keys from `En_pub`, `Et_priv` and `s`, opens the request and shows the
   device name and a short fingerprint of `sign_pub` to the user.
5. On approval `T` writes the signed device record (`crypto.md` 7.4) and
   `inbox/<invite_id>.resp = seal(K_resp, cbor({secret_key, device_id}), aad("invite-resp", vault_id,
   invite_id))`.

Properties:

- A storage attacker sees `Et_pub`, `En_pub` and sealed blobs but not `s`, so cannot derive `K_req` or
  `K_resp` and cannot read the device name or the Secret Key. An attacker who swaps `Et_pub` learns nothing
  for the same reason, and `N` simply fails to open the response.
- `s` is shown only on `T`'s screen and in the QR, never stored in the vault or logs, and is 100 bits, so
  guessing it online or offline against the files is infeasible in the 10-minute lifetime.
- The `.resp` contains the Secret Key but not the master password and not `VK`. Stealing it needs `K_resp`.
- The master password is still required on `N`. A thief who intercepted everything still lacks the password.

### 3.4 Rules

- An invite lives 10 minutes (`expires_at`), is single use, and `T` deletes the `.inv` as soon as a valid
  `.req` arrives or the invite expires. Only one invite can be open at a time per device.
- `T` MUST require a biometric or master password confirmation before approving, and show the requesting
  device's name, platform and key fingerprint. Approval never happens automatically.
- Anyone can write files to `inbox/` (the storage allows it), so every file there is untrusted until it
  opens under the right key. Malformed, expired or unrelated files are deleted by `T` and ignored, and
  files older than 24 hours are cleaned up by any device.
- `N` MUST NOT accept a `.resp` whose `device_id` does not match its own, or a device record that does not
  verify against the authority key derived after unlock.
- The `inbox/` directory is also used for shares in a later phase; those get their own file extension.

### 3.5 Pairing without a trusted device

If no trusted device is available, the user types the master password and the Secret Key from the
Emergency Kit on `N`. `N` derives `AUK` and unwraps `VK`, derives the authority key, creates its own device
record and signs it (any holder of `VK` can). The user is told the vault now has a new device and sees it
in the device list on the next sync, from where it can be revoked.

### 3.6 Offline and edge cases

- `T` and `N` do not need to be on the same network, only the same storage. Time between steps is bounded
  by the 10-minute expiry.
- If storage is slow, `N` polls every 3 seconds for up to 2 minutes and then shows "Waiting for the other
  device".
- Two devices claiming the same `device_id`: `device_id` is random per device; a collision (2^-128) is
  treated as a fork (`sync.md` 8) and the later one is rejected.

## 4. Revoking a device

1. The user picks "Remove device" on any unlocked device.
2. It confirms by name and warns that the removed device keeps a copy of everything it already synced
   (`crypto.md` 9.1, threat model D8).
3. The device rotates `VK` (new `key_epoch`), re-signs `vault.json` with the device in the `revoked` list and
   the `revoked_at` stamp, rewraps the keys and writes a snapshot (`sync.md` 9).
4. Every other device is asked for a full unlock on its next sync, because its quick-unlock `AUK` wrap is
   still valid but it needs the new wrapped `VK`; the master password is not changed unless the user chose
   so. It is recommended after losing a device.
5. If the lost device may have been unlocked, the user SHOULD also change the master password and rotate the
   Secret Key (`crypto.md` 9). The UI offers this in the same flow.

## 5. Emergency Kit

The kit is a printable PDF or a file the user can save, produced at vault creation and whenever the Secret
Key is rotated (FIG-31).

Contents:

- Title, product name and a version marker `FL-KIT-1`.
- The Secret Key in the display format of `crypto.md` 4.1, in the mono font with groups, plus a QR code of
  the same string.
- The vault name, the storage in use (for example "Google Drive, account name") and the creation date, so
  the user knows what the kit belongs to. No master password, no `VK`, no other key.
- Plain-language steps: install FigLock, choose the storage, sign in, type the master password, type or scan
  the Secret Key.
- A blank line labelled "Master password (optional, write it elsewhere)" with a warning against storing it
  with the kit.
- A clear sentence that anyone with the kit and the password can open the vault, and that FigLock cannot
  recover a lost password or Secret Key unless a Recovery Key was set up.

Forced verification (MUST, vault creation and each new kit):

1. After the kit is shown or saved, the user is asked to re-enter the last two groups of the Secret Key from
   the saved or printed copy. Copy and paste is allowed, because the user may check a PDF.
2. The vault does not finish creation until this passes. "Skip" exists but leaves a banner "Emergency Kit
   not confirmed" in the app, and quick unlock is not offered until confirmed.
3. A new kit invalidates the old one when the Secret Key rotates; the app says so and lists the date.

The kit file never leaves the device on its own: no cloud upload, no analytics, no copy to the clipboard
except by explicit user action, and the clipboard clears after 30 seconds.

## 6. Recovery cases

| # | Situation | Recoverable? | How |
| - | --------- | ------------ | --- |
| R1 | Forgot the master password, a paired device is unlocked or has quick unlock | Yes | On `T`: Settings > Change master password. The user proves presence with biometrics; `T` rewraps `VK` under a new `AUK` (`crypto.md` 9). No data re-encrypted. Optionally rotate the Secret Key and make a new kit. |
| R2 | Forgot the master password, no usable device, has the Emergency Kit, Recovery Key set up | Yes | Use `RK` (section 7): recover `VK`, set a new master password and Secret Key, publish a new kit |
| R3 | Forgot the master password, no usable device, no Recovery Key | **No** | The `VK` is only wrapped under `AUK`; without the password it cannot be derived. The app says this at setup and in the kit. |
| R4 | Lost the Secret Key, knows the master password, has a paired device | Yes | On `T`: show the Secret Key (biometrics), or rotate it and make a new kit |
| R5 | Lost the Secret Key, knows the password, no device, no Recovery Key | **No** | Same reason as R3 |
| R6 | Lost every device, has password and Emergency Kit | Yes | Install FigLock on a new device, sign in to the storage, type the password and the Secret Key (section 3.5), and optionally revoke the lost ones |
| R7 | Storage account lost or wiped | Yes, if any device is left | Any device re-uploads its local copy (`sync.md` 8, rollback); with no device and no copy it is lost |
| R8 | Device stolen | Yes | Revoke it (section 4), rotate the password and Secret Key if it was unlocked |

The app shows these cases in plain words in the setup flow ("What happens if I forget my password?") so
the user decides about the Recovery Key knowingly.

## 7. Recovery Key and Shamir split (optional)

For users who cannot accept R3 and R5. It is off by default and offered during setup and in Settings.

### 7.1 Construction

`RK` is a long-term X25519 key pair `(rk_priv, rk_pub)`.

- `vault.json` stores `rk_pub` and `wrapped_vk_rk = seal_to(rk_pub, VK, aad("vk-wrap-rk", vault_id, u32(key_epoch)))`.
- `seal_to(pub, plaintext, aad)` generates an ephemeral X25519 pair, computes `shared = X25519(eph_priv, pub)`,
  derives `key = HKDF(shared, salt = eph_pub || pub, info = "figlock/v1/seal-to", L = 32)` and outputs
  `eph_pub || seal(key, plaintext, aad)`.
- Because wrapping needs only the public half, any unlocked device can rewrap `VK` for `RK` when `VK`
  rotates (`crypto.md` 9.1) without knowing `rk_priv`. This resolves the open question in `crypto.md`: `RK`
  wraps `VK`, not `AUK`, and survives password changes.
- `rk_priv` (32 bytes) is shown once and never stored by FigLock. It is presented in one of two forms:
  a single code (`FLR1-` + Crockford base32 with a check, like the Secret Key) or a Shamir split.

### 7.2 Shamir split

Shares use **SLIP-0039** (Shamir's secret sharing for mnemonic codes, with its published test vectors),
not a custom scheme.

- The secret is `rk_priv` (256 bit). One group, threshold `M` of `N`, with `2 <= M <= N <= 16`. Default
  2 of 3. Passphrase empty (the shares are not protected by a second factor in v1).
- Each share is a 33-word mnemonic with a built-in checksum, printed on its own card with the vault name,
  share number, threshold and a warning. Cards carry no other secrets.
- FigLock never stores shares and never sees them again after display. A "verify" step has the user re-enter
  one word chosen at random from each card before the setup finishes, so unreadable cards are caught.
- Fewer than `M` shares reveal nothing about `rk_priv` (information-theoretic, property of the scheme).

### 7.3 Using the Recovery Key

1. User chooses "I forgot my password" on a fresh install, selects the storage and "Use Recovery Key".
2. They type the single code, or `M` shares. The app reconstructs `rk_priv` and opens `wrapped_vk_rk`.
3. It derives the authority key, shows what it found (vault name, number of items, last change), and asks
   for a **new master password**. A new Secret Key is generated, `AUK` derived, `VK` rewrapped, `vault.json`
   re-signed, and the old Secret Key is invalidated. A new Emergency Kit is produced and verified.
4. It offers to rotate `RK` (new key pair, new shares) because the old ones were just used on a device the
   user may not fully trust; this is the default.
5. It recommends revoking all other devices and re-pairing them, since whoever held the old credentials may
   still hold them.

### 7.4 Risks stated to the user

- Anyone who gathers `M` shares (or has the single code) can open the vault without the password. Shares
  should be held by different people or places. Shamir does not protect against a holder who loses a share
  and a thief who finds it, only against too few being gathered.
- Losing more than `N - M + 1` shares loses recovery, not the vault.
- The Recovery Key does not replace a good Emergency Kit; it covers the case where the password is lost.

## 8. Failure and abuse handling

- **Brute force of unlock.** There is no server to rate-limit, so cost is carried by Argon2id and the Secret
  Key (`crypto.md` 5). Local UI delays apply after 5 failed attempts on one device (increasing up to 10
  minutes) to blunt shoulder-surfing, not offline attacks.
- **Phishing the invite code.** The code is useless without the storage account and is only accepted by
  FigLock apps; the docs say never to read it out or type it into a website.
- **Malicious `.inv` or `.req` files in `inbox/`.** Treated as untrusted input: size limits (16 KiB),
  strict CBOR parsing, fuzzed (`sync.md` 12), and never applied without a valid seal.
- **A kit photographed.** The Secret Key alone is not enough without the password; the app says so and
  recommends rotating the Secret Key if a kit is lost.

## 9. Required tests and vectors (FIG-20)

1. Invite: `invite_id`, `K_req`, `K_resp` from fixed `s`, `Et`, `En`; request and response seal/open; wrong
   `s` fails; swapped `Et_pub` fails; expired and reused invite refused.
2. Emergency Kit content check (no master password, correct Secret Key encoding); verify step accepts and
   rejects the right and wrong groups.
3. `seal_to` round trip; rewrap of `VK` for `rk_pub` after rotation without `rk_priv`.
4. SLIP-0039: the official vectors for every `M`-of-`N` accepted and rejected combination used by FigLock,
   plus 2-of-3 and 3-of-5 round trips of a real `rk_priv`.
5. Recovery R1, R2, R6 end to end in a test harness with two simulated devices; R3 and R5 fail with the
   documented error and no partial state.
6. Revocation: revoked device cannot open files of the new epoch; kept data stays readable by others.

## 10. Open questions

- A passphrase on Shamir shares (SLIP-39 supports one). Default empty in v1; decide after usability tests.
- Whether the invite should also carry the storage account hint (for example the Drive account name) so `N`
  can warn on a mismatch. Left out of v1 to keep the invite minimal.
- A "heir" or time-delayed recovery for the death of the owner is out of scope; Shamir cards cover it by
  hand for now.
