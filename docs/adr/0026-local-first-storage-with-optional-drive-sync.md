# ADR-0026: The vault always lives in a local folder; Google Drive is an optional synced second location

- Status: Accepted (amends ADR-0010 and ADR-0021)
- Date: 2026-11-12
- Decision: D26 in `docs/decisions.md`

## Context

FIG-88 slices 1 and 2 let a device keep its vault in a folder OR in Google Drive. A Drive vault then has no
local copy and nothing keeps two locations matched. That breaks ADR-0010 ("a full local copy on every device
is required", because disconnecting the app can delete the hidden Drive folder) and makes it impossible to
start with a plain folder and add Drive later. The maintainer decided the structure is always a local folder,
with Drive as an optional second location synced in the background.

## Decision

- **Every device has a local vault folder.** The person picks it, or, when they choose Drive without picking
  one, it is `<app data dir>/vault`. All reads and writes go to the local folder, so the app and CLI work
  offline and never wait on the network.
- **Drive is an optional remote.** Device config holds the local folder (required) and `remote` (absent or
  `drive`). Choosing Drive at setup means "local copy plus sync", never Drive alone.
- **Turning Drive on later** signs in, then uploads what is already local. **Turning it off** stops syncing
  and leaves the local copy and the Drive data alone (signing out is a separate step, ADR-0021).
- **Opening an existing vault from Drive, or joining a device by pairing, downloads it into a new local
  copy** and then syncs normally.
- **Migration.** A `config.json` with `store_kind: drive` (a Drive-only device) gets a default local folder
  populated from Drive on first run. A `Folder` config is unchanged and simply has no remote.

### Sync algorithm

Sync mirrors two `BlobStore`s (local, remote) as ciphertext, never interpreting contents. It runs pull then
push. The layout (`sync.md` 3) makes this a set union, because every file except two kinds is create-only
and named by its owner:

- **Create-only files** (`log/`, `snap/`, `inbox/`): copy every path present on one side and missing on the
  other with `put_new`; `Exists` means someone else already wrote it, which is fine. Paths are validated and
  sizes capped by the adapters as today. A copied file is only a stored blob: it is verified when the vault
  reads it (`sync.md` 8), so a hostile remote cannot make anything apply that does not verify.
- **`vault.json`** is replaced only when the other side has a higher signed `revision`, never lowered. The
  `devices/` records follow the side that supplied the winning `vault.json` (a rotation re-signs them,
  ADR-0018). A lower remote `revision` is storage going backwards (`sync.md` 8.6): local is kept, a warning
  is raised, and the next push puts the newer copy back.
- **`vault.json` fork.** Two devices that change credentials, revoke a device or rotate keys while apart
  both write `revision + 1`, and the two files differ. This is the one case where sync cannot just union:
  1. A different `revision` is not a fork; the higher one wins as above.
  2. For the same `revision` with different bytes, every device picks the same winner by the lowest SHA-256
     of the file, so both sides converge without coordination. The sync engine reports a fork warning.
  3. The device whose change lost does not silently drop it. On its next unlock it compares the winner with
     what it wrote and re-applies the difference as a new write at a higher `revision`: a revocation is
     re-added to the `revoked` list (revocations only ever grow, so they are never lost to a fork), and a
     key rotation is redone if the winner is at a lower `key_epoch`. A credential change (password or Secret
     Key, ADR-0022) that lost is not merged: the person is told which change was superseded and repeats it.
  4. Op files sealed under an epoch the winner does not know stay readable through `epoch_keys`; if the
     winner lacks that epoch's key, the device re-runs the rotation rather than leaving data unreadable.
  The winner rule is deterministic but arbitrary, so a fork is rare and loud by design: it needs two
  devices making vault-level changes at once, and the UI says so. This rule is checked against the vault
  code in the sync engine slice, and `sync.md` 3 gets a matching line there if it changes.
- **Deletions are not mirrored.** Compaction (`sync.md` 7) is done by the owning device on both sides.
  Pull skips op files already covered by a valid snapshot's high-water marks, so a compacted file is not
  pulled back from a stale remote.
- **Same vault only.** A remote whose `vault.json` has a different vault id from the local one is refused,
  never merged.
- **Schedule** follows `sync.md` 10: every 15 seconds while the app or broker runs, plus after a local write,
  on unlock, and on manual sync; backoff 5 s, 15 s, 60 s, then 5 minutes.
- **One sync at a time per device.** A sync holds an exclusive lock file (`sync.lock` in the config home) for
  its whole run; a second sync fails fast as busy (silent on the automatic paths) instead of mirroring too.

### Failure modes

| Situation | Behaviour |
| --- | --- |
| Offline or Drive 5xx | local keeps working; status "not synced, last synced <time>"; backoff |
| Sign-in revoked or expired (`Unauthorized`) | local keeps working; status "sign in again"; no retry loop |
| Remote folder deleted or empty while local is not | treated as remote lost; the next push re-uploads everything local, after the person confirms once |
| Remote older than local | push fills the gap |
| Remote newer than local | pull, then verify as usual |
| Remote has files that fail verification | left as stored, ignored by the vault, reported in status |
| Local folder missing or unreadable | the existing "vault folder is not available" error; remote is never used as a substitute |

### CLI

- Setup (CLI and desktop) offers three choices: create a new vault, open an existing vault from Drive, or
  join a device by pairing. Open and join create a local copy and pull what is in Drive.
- `figlock init` creates the local vault at `--store <dir>` (default `<app data dir>/vault`); `--drive` adds
  Drive sync to it. The CLI has no open or join command today; when it gets them they download into a new
  local copy like the desktop app does.
- `figlock sync` runs one pull and push and prints what moved (`--json` supported); `--reupload` is the
  person's confirmation after the remote was lost. `figlock sync on` and `figlock sync off` enable and
  disable the Drive remote (`on` only stays on if the first sync works); `figlock sync status` shows signed
  in, last sync and the local path, with no network call. `figlock drive login` and `logout` are unchanged.
- Ordinary commands read and write the local folder and never depend on the network. Before a command that
  unlocks the vault, the CLI pulls if the last sync is more than 15 seconds old; after a command that writes,
  it pushes. Both have a 5 second limit, are skipped when sync is off, and on failure print a warning (the
  pull stays silent when simply offline) and carry on: the next sync catches up.
- The broker daemon syncs once before it unlocks. A background loop while it runs waits until the broker can
  reload its in-memory vault, because pulling files it never reads would add nothing.

## Consequences

- A full local copy exists on every device, as ADR-0010 requires; losing or disconnecting Drive never loses
  the vault.
- Setup, open and join have one shape, and Drive can be added or dropped at any time.
- Cost: two stores to keep consistent and a sync engine to test (convergence with a flaky remote, rollback,
  deleted remote) and to run on desktop and in the broker.
- No change to crypto, file format or test vectors. `sync.md` 8 already specifies what is verified on read;
  this ADR adds the transport between two stores and needs no spec change.

## Alternatives considered

- Drive-only storage (the FIG-88 stepping stone): no local copy, no offline, breaks ADR-0010. Rejected.
- Local copy as a cache of an authoritative remote: makes Drive required and a deleted folder fatal. Rejected.
- Mirroring deletions: a stale or hostile remote could remove files a device still needs. Rejected.
