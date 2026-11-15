# ADR-0035: Deleting a vault's copy from Google Drive is a deliberate, typed action

- Status: Accepted
- Date: 2026-11-15
- Decision: D35 in `docs/decisions.md`
- Amends: ADR-0026

## Context

ADR-0026 says turning sync off and signing out of Google leave the Drive copy alone (no mirrored deletes). The
copy lives in FigLock's hidden app folder (`drive.appdata`, ADR-0010), which Google Drive's own screens do not
show, so a person who wants it gone had only Google's "Manage apps, Delete hidden app data". That wipes
everything the app stored, for every vault, and is hard to find. Found in FIG-121 and decided in FIG-122.

## Decision

1. **One deliberate action removes this vault's Drive copy:** "Delete from Google Drive" in Settings, Google Drive
   sync (desktop), and `figlock sync delete-remote` (CLI). It needs a Google sign-in, works whether sync is on or
   off, and needs no unlock: it deletes ciphertext only.
2. **What it deletes:** every file in the app folder that belongs to this vault, `vault.json` last, so a run that
   stops half way can be repeated (the remote still says whose it is). A file another device uploads meanwhile is
   swept up in a following pass (three at most), and the result is checked by listing the folder again.
3. **What it never touches:** the vault on this device, the device keys and config, the Secret Key, the Google
   account and anything outside the app folder. It does not revoke the Google access; signing out stays a
   separate step.
4. **Only this vault.** It first reads the Drive `vault.json` and compares `vault_id` with the local one. A Drive
   that holds a different vault is refused (`ForeignVault`), and so is a Drive that has files but no
   `vault.json` (nothing proves whose they are). An empty Drive is a calm no-op.
5. **Hard to do by accident.** The window says exactly what goes and what stays, and the button stays disabled
   until the person types `delete from Google Drive` (case and surrounding spaces ignored). The phrase is
   checked again in the app core and the CLI, so no client can skip it: a call without it deletes nothing. The
   vault name would be typed without thought (every vault is "Personal"), so a phrase is used. The CLI asks for
   the phrase at a terminal and takes `--confirm "<phrase>"` for scripts; with neither it refuses.
6. **Sync ends up off on this device**, so it does not at once offer to upload the vault again. Turning sync on
   later uploads it anew.
7. **Other devices.** No attempt is made to detect or wait for a device that is mid-sync. A device with sync on
   finds `vault.json` gone on its next sync and reports "Vault missing from Drive" (`SyncError::RemoteLost`),
   which asks the person before uploading everything again; nothing is uploaded unasked. A file such a device
   pushed during the deletion is swept as in 2, or stays as clutter that the next deletion removes.
8. Turn off sync and Sign out of Google dialogs now say that this action exists, instead of sending people to
   Google's hidden-data setting.

No crypto, file format or sync-algorithm change: the sync algorithm stays a set union with no mirrored deletes;
this is a separate, explicit operation.

## Consequences

- Anyone with the machine and a Google sign-in can delete the Drive copy after typing a phrase. That is the same
  power Google's own setting gives, and the local copy and every other device's copy survive it.
- A device that is offline for a long time and syncs after the deletion asks to upload again; if the person says
  yes, the vault is back in Drive. That is the intended recovery.
- Tests: the deletion removes every remote file and nothing local, refuses another vault, a missing local vault
  and an unprovable remote, sweeps a late upload, is repeatable, and a second device sees the vault missing
  and uploads nothing by itself (`crates/figlock-session/src/sync.rs`); no phrase, no sign-in and a script without
  `--confirm` change nothing (`crates/figlock-cli/tests/drive.rs`); the app core refuses without the phrase
  (`apps/desktop/src-tauri/src/desk/removed_tests.rs`); the dialog and its states (`apps/desktop/e2e`).

## Alternatives considered

- **Type the vault name:** rejected, see 5.
- **Refuse while another device could be mid-sync:** there is no reliable signal for it (the folder has no
  presence record), so a refusal would be a guess; the documented outcome in 7 is the right one either way.
- **Delete in one request:** Drive has no "delete folder contents" for the app folder, only per file; the passes
  and the final listing cover it.
- **Also delete the local vault:** never; the person keeps their data, and deleting it is a separate decision.
