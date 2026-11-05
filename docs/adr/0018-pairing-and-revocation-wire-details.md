# ADR-0018: Pairing and revocation wire details

- Status: Proposed
- Date: 2026-11-05
- Decision: D18 in `docs/decisions.md`

## Context

Implementing the invite flow and device revocation (FIG-33) showed gaps and one conflict in the specs:

- The pairing request has no device id, yet `T` must write `devices/<device_id>.json` and `N` must check the
  response against "its own" id.
- The `.inv` encoding, the device fingerprint and the shape of the `revoked` list and `epoch_keys` in
  `vault.json` are named but not defined.
- `sync.md` 3 said device records are created once and never edited, while `crypto.md` 9.1 says a rotation
  rewrites them. The authority key is derived from `VK`, so a rotation invalidates every old signature.

## Decision

- The request plaintext is `{device_id, device_name, sign_pub, agree_pub}`. The invite file is JSON with
  exactly `v`, `vault_id`, `invite_id`, `et_pub`, `expires_at`.
- The fingerprint is the first 10 bytes of `SHA-256("figlock/v1/device-fingerprint" || 0x00 || sign_pub)` in
  Crockford base32, shown as four groups of four.
- `vault.json` gains optional `revoked` and `epoch_keys` members (`crypto.md` 9.2). They are omitted while
  empty, so epoch 1 vaults and their vectors do not change.
- A key rotation replaces `devices/<id>.json` with records re-signed by the new authority key. Readers ignore
  records that do not verify until they are rewritten.
- A rotation does not change the password or Secret Key. The UI follows a revocation with that offer
  (`pairing-recovery.md` 4).

## Consequences

- `pairing.json` and `signatures.json` get new cases; the request vector now carries `device_id`.
- The BlobStore needs `put_replace` on `devices/`, which every adapter already has.
- A revoked device that holds the credentials can still unwrap the new `VK`. This was already implied; it is
  now written down in `crypto.md` 9.1.
- Two rotations without an op file in between produce the same snapshot name. The second `put_new` fails with
  `Exists`; the caller writes one op first. A cleaner naming rule can follow if this shows up in practice.

## Alternatives considered

- `T` assigns the device id: contradicts "N refuses a response for another id" and forces a second round trip.
- Keep the authority key stable across rotations: it is derived from `VK` on purpose, so a revoked device
  never keeps signing power.
- Re-sign nothing and verify old records against the old authority: needs every authority key in a chain and
  lets a revoked device's record stay trusted.
