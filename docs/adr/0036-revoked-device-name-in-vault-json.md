# ADR-0036: A revoked device keeps its name, sealed in the signed `revoked` list

- Status: Accepted
- Date: 2026-11-16
- Decision: D36 in `docs/decisions.md`
- Amends: ADR-0034 (the min_format_version rule), `crypto.md` 9.2

## Context

Revoking a device deletes its record, and the record is where the name lives. The signed `revoked` list keeps
only the id, the signing key, the time and the epoch (`crypto.md` 9.2). So after a revocation every device, the
one that did it included, lists the row as "(name unknown)" (`figlock device list`, the desktop Devices list).
A person with three Macs cannot tell which one was removed. Found in FIG-121 (second run) and decided by the
maintainer in FIG-138.

Storing the name in clear in `vault.json` would show the storage provider a device name, which `AGENTS.md`
forbids (ciphertext only). Remembering names in each device's own config would leave a device that never saw the
record with no name, and add state that is not in the vault.

## Decision

1. A `revoked` entry may carry one more member, `name`: the device name sealed with the Log Key of the entry's
   own `epoch` (the epoch the revocation produced), `seal(LK_epoch, name, aad("revoked-name", vault_id,
   device_id, u32(epoch)))`, hex. Later rotations keep the entry as it is; a reader opens it with the key of that
   epoch, which it has (current, or from the `epoch_keys` chain).
2. A revocation writes `name` from the record it is about to remove. Entries written before this ADR have no
   `name` and stay valid; the row shows "(name unknown)" for them.
3. A file with a `name` in any entry is `min_format_version` 2, as for the Recovery Key and `sk_gen`, so a
   client that predates it refuses the file instead of dropping the names when it rewrites it.
4. A name that does not open is treated as no name: the entry is signed, and a missing label is not worth
   refusing to open the vault.
5. Revoked devices can read the new `vault.json` only with a full unlock, so they learn their own name no
   differently than before; nothing new reaches a revoked device that it did not already have.

## Consequences

- `crypto.md` 9.2 and `pairing-recovery.md` 4 change; vectors `rotation.json` gain the seal, open and two
  rejects (`generate.py`).
- Vaults that revoked a device before this change keep "(name unknown)" for that row.
