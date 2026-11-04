# FigLock sync specification

Status: draft v1 (FIG-17). Defines how devices share a vault through dumb storage with no server: the
storage contract, file layout, op format, the clock, merge rules, snapshots and compaction, and how
rollback, withholding and revoked devices are handled. Encryption, signing and key rules are in
`crypto.md`; pairing and recovery are in `pairing-recovery.md` (FIG-18). Test vectors and property tests
are listed in section 12.

The words MUST, SHOULD and MAY are used as in RFC 2119.

## 1. Design in one paragraph

Each device writes only to its own append-only folder of encrypted, signed **op files**. Nobody else ever
writes there, so dumb storage never sees a write conflict. Every device reads all folders and merges ops
with a deterministic rule (per-field last-writer-wins on a hybrid logical clock), so all devices that have
seen the same set of ops hold the same vault. Periodic **snapshots** let new devices start quickly and let
old op files be deleted.

## 2. Storage contract (BlobStore)

Storage is a flat namespace of paths and blobs. An adapter (local folder, Google Drive appDataFolder,
later Git and S3) implements:

| Operation | Contract |
| --------- | -------- |
| `get(path) -> bytes` | returns the full blob, or `NotFound` |
| `put_new(path, bytes)` | creates the blob if it does not exist; fails with `Exists` otherwise; readers see either nothing or the whole blob, never a partial write |
| `put_replace(path, bytes)` | replaces a blob atomically; used only for files named in section 3 as replaceable |
| `list(prefix) -> [(path, size, mtime)]` | may be stale (eventual consistency) but MUST eventually include every completed write |
| `delete(path)` | removes a blob; deleting a missing path succeeds |

Adapters MUST NOT interpret contents. Atomicity on a local folder is done by writing a temporary file and
renaming it. `mtime` is informational only and never used for ordering.

Provider notes:

- **iCloud Drive and other synced folders.** A file listed but not yet downloaded is "not available yet",
  not "missing". The adapter asks the OS to download it and the sync engine retries; it is never treated as
  a gap (section 8).
- **Google Drive.** Names are not unique in Drive, so the adapter keeps a path-to-file-id map and creates
  files with a create-only request. If duplicates of one path appear, the adapter keeps the oldest, ignores
  the others and warns; byte-identical duplicates are harmless.

## 3. Layout

```
figlock/
  vault.json                         replaceable. KDF params, wrapped keys, key epoch, revoked list (signed)
  devices/<device_id>.json           created once by the authority; revocation edits vault.json, not this file
  log/<device_id>/<seq>.op           op file, create-only. <seq> is 8 decimal digits, 00000001 upward
  snap/<device_id>-<seq>.snap        snapshot, create-only. <seq> is the writer's own op seq it covers through
  inbox/<request_id>.req             pairing requests and shares (FIG-18), create-only
```

- `vault.json` is the only file that is replaced in place (password change, key rotation, revocation). It
  carries a monotonically increasing `revision` and is signed (`crypto.md` 7.4). A reader that sees a lower
  `revision` than it has already accepted ignores the older copy and raises a rollback warning.
- A device only ever writes inside `log/<its own device_id>/`, its own `snap/` names, `inbox/` and, if it is
  holding the authority key, `devices/` and `vault.json`.
- Paths use lower case hex and digits only, so they survive case-insensitive file systems.

## 4. File container

Op files and snapshots share one container. All integers are big-endian.

```
offset  size   field
0       2      magic "FL"
2       1      container version 0x01
3       1      kind: 0x01 op file, 0x02 snapshot
4       4      key_epoch
8       4      length N of the envelope
12      N      envelope        (crypto.md section 3: 0x01 || nonce || ciphertext || tag)
12+N    64     Ed25519 signature (crypto.md 7.3), covers the 8-byte header (bytes 0 to 7) and the envelope
```

`header` in `crypto.md` 7.3 is bytes 0 to 7 (magic, version, kind, `key_epoch`). The AAD for the envelope is
`aad("log", vault_id, device_id, u64(seq), u32(key_epoch))` for op files and
`aad("snap", vault_id, device_id, u64(seq), u32(key_epoch))` for snapshots.

A reader MUST reject, without applying anything: wrong magic or unknown version, a `device_id`/`seq` that
does not match the path, a device that has no valid record or is revoked for that epoch (section 9), a bad
signature, and a failed `open`.

## 5. Clock: hybrid logical clock (HLC)

Wall time alone fails when device clocks are wrong; counters alone lose the link to real time. FigLock uses
a hybrid logical clock stamp of 8 bytes:

```
hlc = u48(wall_ms) || u16(counter)                 # 8 bytes, big-endian
total order key = (wall_ms, counter, device_id)    # the 16-byte device_id breaks ties
```

Each device keeps `last = (l, c)`, persisted locally.

- **Local event** (a write): `now = wall_ms()`. If `now > l` then `l = now, c = 0` else `c = c + 1`.
  If `c` would exceed `0xFFFF`, wait one millisecond and retry. The stamp used is `(l, c)`.
- **Receiving a stamp `(lm, cm)`**: `l' = max(l, lm, now)`. If `l' == l == lm` then `c = max(c, cm) + 1`;
  else if `l' == l` then `c = c + 1`; else if `l' == lm` then `c = cm + 1`; else `c = 0`. Then `l = l'`.
- **Drift bound.** An op whose stamp has `wall_ms` more than 24 hours ahead of the local clock is
  **held**, not applied, until the clock catches up, and the UI tells the user a device's clock looks wrong.
  Nothing a faulty or hostile device writes can move the clocks of other devices more than 24 hours.
- **Stamps never repeat per device**: `(l, c)` strictly increases for one device, including across restarts
  (persisted `last` is raised to at least `now` on start).

The comparison of two writes is by the total order key; the larger wins.

## 6. Data model and operations

A vault is a set of **items**. An item is a set of **fields**, each a **register** that stores
`(hlc, device_id, value)`. The state of an item is the map from `field_id` to its winning register.

`field_id` is a short ASCII name chosen by the schema, for example `title`, `username`, `password`,
`notes`, `provider`, `env_var`, `tags/<tag>`, `url/<uuid>`, `custom/<uuid>/name`, `custom/<uuid>/value`.
Multi-valued data is stored as many fields with unique ids, so concurrent additions never collide:
adding a second URL is writing `url/<new uuid>`; removing it is writing the field's null value.

Special fields:

| Field | Meaning |
| ----- | ------- |
| `_deleted` | boolean register; `true` means the item is in the trash / deleted. Edits to other fields do not change it. |
| `_vault` | which vault (Personal, Work) the item lives in |
| `_type` | item type, set at creation |

### 6.1 Ops

An op file payload (before sealing) is deterministic CBOR:

```
{
  "v": 1,
  "device": <16 bytes>,
  "seq": <u64>,
  "prev": <32 bytes>,        # SHA-256 of the previous op file (full container) of this device; 32 zero bytes for seq 1
  "ops": [ Op, ... ],        # in the order they were made, stamps strictly increasing
  "pad": <zero bytes>        # padding, section 11
}
```

An `Op` is one of:

| Op | Fields | Effect |
| -- | ------ | ------ |
| `item.create` | `item` (id), `hlc`, `wrapped_ik` | registers the item and its wrapped key; idempotent |
| `field.set` | `item`, `field`, `hlc`, `blob` (the `field_blob` of `crypto.md` 7.1), or `blob = null` for removal | register write |
| `item.delete` | `item`, `hlc` | same as `field.set` of `_deleted = true` |
| `item.restore` | `item`, `hlc` | same as `field.set` of `_deleted = false` |

`item.create` MUST precede, in the same or an earlier file of the same device, any `field.set` of that item
by that device. A device that sees a `field.set` for an item whose `item.create` it has not seen yet keeps
the op pending until the create arrives (it will, from the creator's folder).

### 6.2 Merge rule

Applying a set of ops is a pure function of the set, independent of order and repetition:

```
for each (item, field): value = the write with the greatest (wall_ms, counter, device_id)
```

Properties this gives, which are required and tested (section 12):

- **Commutative and associative:** any order of arrival gives the same state.
- **Idempotent:** applying an op twice, or applying an op already folded into a snapshot, changes nothing.
- **Convergent:** devices that have applied the same set of ops hold the same state.

Consequences users should know and the UI states plainly: if two devices edit the same field of the same
item at nearly the same time, the later stamp wins and the other value is not merged but stays
available in item history (the "previous values" feature in the PRD). Edits to different fields merge.
Long notes are one field; concurrent edits of the same note keep one version and the other is recoverable
from history.

### 6.3 Deletion

Deleting is writing `_deleted = true`. It wins over concurrent edits to other fields, because edits do not
change `_deleted`. Editing a deleted item does nothing visible until it is restored. Restoring writes
`_deleted = false` with a later stamp. A delete and a restore race by the same rule as any field.

### 6.4 Purge

Items whose `_deleted` has been true for more than 90 days MAY be purged by a snapshot writer (section 7):
the item is omitted from the snapshot and the snapshot records `purge_horizon = hlc of the snapshot`. Op
files for it can then be deleted. An op for an unknown item whose stamp is older than `purge_horizon` is
dropped, so a stale device cannot resurrect a ghost item.

## 7. Snapshots and compaction

A snapshot is the full merged state at a point in time:

```
{
  "v": 1, "device": <16>, "seq": <u64 of writer's own op seq covered>,
  "hw": { <device_id>: <highest seq included>, ... },     # high-water marks
  "purge_horizon": <hlc>,
  "items": [ { "item", "wrapped_ik", "fields": { field_id: [hlc, device_id, blob|null], ... } }, ... ],
  "pad": <zero bytes>
}
```

- Any device MAY write a snapshot. A device SHOULD after it has applied 500 ops since the last snapshot or
  7 days, whichever is first, and after a key rotation (`crypto.md` 9.1, mandatory).
- Applying a snapshot is a merge: each register is folded in by the merge rule, so it is safe to apply on
  top of existing state and safe to apply twice.
- **Starting a new device.** Load `vault.json`, `devices/`, then the snapshot with the greatest `hw`
  (compare by dominance; if none dominates, apply all valid ones), then read op files with `seq > hw[device]`
  for every device.
- **Compaction.** A device MAY delete its own op files with `seq <= hw[self]` of a snapshot that is at least
  30 days old and that it has verified. It keeps its latest two snapshots and deletes older ones that
  another valid snapshot dominates. Nobody deletes another device's files.
- **Long-offline device.** If a device was offline long enough that op files it needs are gone, it loads the
  newest snapshot, then re-submits its own unsent local ops as new files. Because merge is idempotent and
  per-field, this cannot lose newer data, and `purge_horizon` blocks resurrection of purged items.

## 8. Reading storage safely

The sync engine reads in a fixed order and applies only what verifies:

1. `get(vault.json)`. Verify the signature after unlock (`crypto.md` 7.4). Check `revision` is not lower
   than the accepted one. Adopt new `key_epoch`, KDF parameters and the revoked list.
2. `list(devices/)`, verify each record against the authority key. Unknown or unsigned records are ignored.
3. For each valid, non-revoked device `D`: `list(log/D/)`, take files with `seq > applied[D]`, process
   **strictly in order** `applied[D] + 1, +2, ...`. For each: verify container and signature, check `prev`
   equals the hash of the previously accepted file, decrypt, apply.
4. **Gap.** If `seq = applied[D] + k` exists but `applied[D] + 1` is missing, do not apply anything for `D`
   beyond the gap. Retry on the next poll; after 10 minutes show "waiting for a missing file from
   <device>". Files covered by a newer valid snapshot are not a gap.
5. **Fork.** Two different valid files for the same `(D, seq)`, or a `prev` mismatch, mean a device's log was
   forked, rolled back, or a key was cloned. Stop applying `D`, keep what was already applied, and warn the
   user, naming the device.
6. **Rollback.** The engine persists `applied[D]` and the highest `revision`. Seeing less than that (a
   listing that shrinks, an older `vault.json`) is storage going backwards. Never lower local state; warn,
   and keep a full local copy. Because every device has the full local state, an honest device can restore
   storage by re-uploading its own files and a new snapshot.
7. Ops with a stamp more than 24 hours in the future are held (section 5).

A storage attacker can still delete or withhold files, or present a stale but internally consistent
history, to a device that has never seen anything newer. This is detected the moment any device that has
seen newer state syncs, and is listed in the threat model (S5, S6). It cannot forge, alter, or reorder ops.

## 9. Revoked devices and key epochs

- Revoking a device rotates `VK` (`crypto.md` 9.1) and writes the device and its `revoked_at` stamp into the
  signed `revoked` list in `vault.json`, along with the new `key_epoch`.
- A revoked device's op files are accepted only if their stamp is earlier than `revoked_at` **and** they
  were written in an epoch before the rotation. Anything later is ignored.
- After a rotation the old epoch's op files stay readable through `epoch_keys` until a snapshot sealed under
  the new epoch covers them; after that, old files can be compacted away.
- A device that is offline during a rotation finds, on return, a higher `key_epoch` in `vault.json`, asks for
  a full unlock (the wrapped new `VK` opens under `AUK`), and continues. If it was the one revoked it
  simply cannot decrypt anything newer.

## 10. Schedule and latency

- While the app is open: poll `vault.json` and the log folders every 15 seconds, and use the local file
  watcher or the Drive changes feed where available. New remote changes MUST be visible within 30 seconds
  (PRD NFR).
- Local writes are batched into one op file at most every 2 seconds while editing, immediately on lock,
  quit and sleep, and never split a single user action across files.
- Backoff on errors: 5 s, 15 s, 60 s, then every 5 minutes; resume on the next manual sync.

## 11. Size, padding and the storage provider's view

The provider sees file names, counts, sizes and times. To limit what sizes reveal, the CBOR payload is
padded with zero bytes before sealing to the next size in `{1, 2, 4, 8, 16, 32, 64, 128, 256, 512} KiB`, then
to the next multiple of 1 MiB. Device ids and sequence numbers are visible in paths, so the provider can
learn how many devices exist and how often each writes. This is accepted (threat model S7).

## 12. Required tests and vectors (FIG-20, FIG-26)

Test vectors (fixed files with expected state):

1. Container encode and decode; every rejection in section 4.
2. HLC: local events, receive rules, counter overflow wait, drift hold, restart persistence.
3. Merge: same field, different fields, delete versus edit, delete versus restore, tie broken by `device_id`.
4. Snapshot: applying twice, on top of newer state, and purge-horizon dropping a ghost op.
5. Gap, fork, rollback, held future op, revoked device before and after `revoked_at`.

Property tests (`proptest`, in `figlock-core`):

- For random sets of ops from random devices, every permutation and every duplication of delivery order
  gives the same final state (convergence, commutativity, idempotence).
- State after `snapshot(state) + remaining ops` equals state from replaying all ops.
- Interleaving local writes with remote receives never makes a device's own stamps decrease or repeat.
- No sequence of valid ops makes a field value decryptable without the right `IK`.

Fuzzing (`cargo-fuzz`): the container parser, the CBOR payload decoder and the path parser.

## 13. Open questions

- Whether `tags/<tag>` should be an observed-remove set instead of per-field registers (matters if many
  concurrent tag edits show up in practice; v1 keeps registers for simplicity).
- Snapshot cadence defaults (500 ops, 7 days) to be tuned with real vault sizes in FIG-26.
- Git adapter (later): `put_new` maps to commit, and `list` to a tree walk; to be specified when built.
