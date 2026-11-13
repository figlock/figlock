# ADR-0029: Recovery Key code format and vault.json members

- Status: Accepted
- Date: 2026-11-13
- Decision: D29 in `docs/decisions.md`

## Context

The Recovery Key (`pairing-recovery.md` 7) is specified as an X25519 key whose public half wraps the Vault Key in
`vault.json`, and whose private half is written down as one code or as SLIP-0039 shares. The wrap itself
(`seal_to`, `wrapped_vk_rk`) has vectors since FIG-20. Three things were left open and are needed to build it
(FIG-103): the exact text of the single code, how `vault.json` carries the wrap through every rewrite, and which
operations may change it.

## Decision

- **Code.** `FLR1-` followed by 14 groups of 4 Crockford base32 characters: `rk_priv` (32 bytes) and a 3-byte
  check, `SHA-256("figlock/v1/rk-check" || rk_priv)[0..3]`. Three check bytes are the smallest that make the
  total a whole number of base32 groups (35 bytes, 56 characters). Parsing follows the Secret Key rules, and a
  Secret Key is refused. Vectors: `recovery-key.json` (`crypto.md` 4.2).
- **`vault.json` members.** `rk_pub` and `wrapped_vk_rk`, both or neither, inside the signed body, with
  `min_format_version` 2 while they are present (`crypto.md` 9.3). A rotation of `VK` rewraps for the same `rk_pub` in the same write; password and Secret Key
  changes leave them alone.
- **Who may change it.** Enabling or replacing the Recovery Key needs the master password and Secret Key, as a
  credential change does, because the unlocked vault does not keep `VK` (it is opened from `wrapped_vk` and sealed
  to the new public key). Removing it needs neither. The app also asks for Touch ID or the master password
  before any of these.
- **Recovery.** Opens `wrapped_vk_rk` with `rk_priv`, then follows the unlock checks (authority key derived from
  `VK`, signature, `authority_pub`) and writes a new `vault.json` under a new master password and a new Secret
  Key in one write, optionally with a new Recovery Key.

## Consequences

- The vault format gains two optional members. A vault with a Recovery Key has `min_format_version` 2, so an
  older client refuses to open it rather than rewriting `vault.json` without the members and silently dropping
  the Recovery Key. A vault without one is byte for byte as before, at version 1.
- Replacing the Recovery Key makes old cards useless from the next `vault.json`; a copy of an older
  `vault.json` can still be opened by the old key. The UI says so when the key is replaced.
- Recovery does not revoke other devices. The flow recommends it (`pairing-recovery.md` 7.3 step 5).
