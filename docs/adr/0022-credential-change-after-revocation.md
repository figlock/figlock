# ADR-0022: Credential change after revocation

- Status: Accepted
- Date: 2026-11-06
- Decision: D22 in `docs/decisions.md`

## Context

Revoking a device rotates `VK` (ADR-0018), but the wrapped `VK` in `vault.json` is still opened by the same
`AUK`. A revoked device that holds the master password and Secret Key can therefore unwrap the new `VK` from
the current `vault.json`. `crypto.md` 9 already lists password change and Secret Key rotation as a rewrap of
`VK` under a new `AUK`; nothing implemented it (FIG-63).

## Decision

- `Vault::change_credentials` implements the first three rows of `crypto.md` 9: check the current
  credentials, fresh salt, optional new Secret Key and Argon2 parameters, rewrap the same `VK`, replace
  `vault.json` at `revision + 1`. No new primitive, no format change, so no new test vectors; the behaviour is
  covered by `tests/credentials.rs`, including a revoked device with the old credentials that cannot unlock.
- It does not rotate `VK`. After a revocation the caller runs it before any new item is written. A revoked
  device holding the credentials and a copy of the pre-change `vault.json` can still open that epoch, which
  is the limit already stated for rotation (`crypto.md` 9.1).
- The two operations stay separate. Making every password change rotate `VK` would rewrite the whole vault
  for a routine change, and a combined "revoke and change credentials" flow can be built from the two calls.

## Consequences

- Other devices fail their next quick unlock and ask for a full unlock with the new credentials. The CLI
  must call `Quick::forget` on a change and offer the new Emergency Kit when the Secret Key rotated.
- The Recovery Key wrap of `VK` (`pairing-recovery.md` 7) is untouched, because `VK` is the same.
- Two revocations with no op file in between still produce the same snapshot name (ADR-0018). A credential
  change writes no snapshot, so it does not add to that problem.
- CLI and app commands for the change are separate tasks.

## Alternatives considered

- Rotate `VK` on every credential change: closes the copied-`vault.json` window but costs a full snapshot
  and an epoch per password change.
- One combined revoke-and-change call: fewer ways to forget the second step, but a larger API to test and
  specify; it can be added on top later.
