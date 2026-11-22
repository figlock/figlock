# ADR-0043: iCloud Drive as a plain folder, and several remotes at once

- Status: Accepted
- Date: 2026-11-22
- Decision: D43 in `docs/decisions.md`
- Amends: ADR-0026 (Drive is the one optional remote), ADR-0041 section 4 (one remote active at a time)
- Task: FIG-194 (decision), FIG-195 and FIG-196 (code)

## Context

ADR-0026 made the local folder the working vault and Drive its single optional remote. ADR-0041 added a picked
iCloud Drive folder as the iPhone's remote slot, still one at a time. Two things changed. People want the vault
in iCloud Drive and Google Drive together, so one failing provider never leaves them without a copy. And on
macOS 14 and newer iCloud Drive runs on File Provider: an evicted file keeps its name and carries the
`SF_DATALESS` flag, and the hidden `.name.icloud` stubs the folder adapter looked for no longer exist.

The iCloud app container (CloudKit or a ubiquity container) was spiked again. A process without the iCloud
entitlement cannot create a container (the `mkdir` in Mobile Documents is denied), container data stops syncing
when the owning app is uninstalled, the CLI cannot carry the entitlement, and public containers are flaky.

## Decision

- **A remote is `drive` or `folder(path)`, and a device has a list of them.** Each is mirrored from the local
  working copy on its own: pull then push, its own back-off, its own same-vault check, its own remote-lost
  confirmation, its own "last synced" and error state. One failing or lost remote never blocks another.
- **The local folder stays the working vault.** iCloud is never the working copy.
- **iCloud Drive is a plain folder** (reaffirms ADR-0002), picked as a `folder` remote. The iCloud app container
  is rejected again. It may return later as an opt-in Apple-only adapter behind the same `BlobStore` trait.
- **A folder remote may not be, contain or sit inside the local vault folder**, compared by real path, when it is
  added and every time it is opened.
- **iCloud is Apple-only.** Headless Linux servers and CI use Drive, and later Git or S3.
- **Dataless files (macOS 14+).** A file with `SF_DATALESS` is listed as "not downloaded yet", never missing and
  never a gap. The adapter asks the OS to download it (Foundation `startDownloadingUbiquitousItem`, which works
  from an unsigned binary; `brctl download` was removed in macOS 14) and the sync engine waits a bounded time
  for `vault.json`, then reports "not downloaded" and retries on the next run. The `.icloud` stub path stays for
  older macOS.
- **Config.** `config.json` holds `remotes`, a list of `"drive"` or `{"folder": "<path>"}`. A config with the old
  single `remote` reads unchanged as a list of one and is written in the new shape on the next change. Sync state
  is kept per remote in `sync-state.json`; the old flat record is Drive's.
- A sync in which neither side holds a vault records nothing, so an empty remote that a device reaches before it
  has its vault is not later taken for a lost one.

## Consequences

- No crypto or file-format change, so no new test vectors. Convergence with two remotes and one flaky is covered
  by a property test.
- Settings screens and the CLI that add a folder remote, and per-remote status in the desktop window, are
  follow-up work. Until then a folder remote is set in `config.json`.
- Two remotes mean two uploads of every change; a remote that is slow only delays itself.

## Alternatives considered

- **iCloud app container.** Rejected for the reasons above.
- **One remote, switchable.** Rejected: it gives no redundancy and a switch loses the history of what each side
  holds.
- **Treat the folder remote as the working copy.** Rejected: a working copy that can be evicted or conflicted by
  another process breaks the offline-first guarantee of ADR-0026.
