# ADR-0024: Document items and per-field size limits

- Status: Accepted
- Date: 2026-11-09
- Decision: D24 in `docs/decisions.md`

## Context

Signing and API credentials are often a file plus a password: a `.p12` and its password, an App Store Connect
`.p8`, a service-account JSON, a TLS key. FigLock had no place for them (FIG-77). Separately, the only size
rule on a field was a 1 MiB ceiling that applied equally to a title and a file, with no limit on how many
fields an item could hold, so any field could be used to bloat a vault and every device that syncs it.

## Decision

- **New item type `doc`.** Fields: the common ones (`title`, `notes`, `tags/<tag>`, `custom/<id>/...`), plus
  `password`, `expires_at`, `url/<id>`, and files as `file/<id>/name`, `file/<id>/mime` and `file/<id>/data`.
  `password` and `file/<id>/data` are secret fields: never indexed, never shown without a reveal. File names
  are searchable. `<id>` is a short unique suffix, so two devices adding files concurrently merge.
- **A file is base64 text in `file/<id>/data`** (RFC 4648 standard alphabet, padding required, decoded
  strictly). Values are CBOR text everywhere else, so there is no new value kind, no change to `crypto.md` 7.1
  or `sync.md`, and no new test vectors. It costs 33% on disk, which is small next to the cap below. A native
  bytes value would need a second value type in every client binding; it is not needed for files this small.
- **A file is at most 768 KiB** (`MAX_FILE_BYTES`), which encodes to exactly the 1 MiB value ceiling. A
  document holds at most 8 files. Larger files are out of scope: they would need separate encrypted blobs
  referenced from the item, which is a different design and ADR.
- **Every field has its own cap**, decided by the field name alone (`max_value_bytes` in `schema.rs`): for
  example title, username and TOTP 1 KiB, password 4 KiB, token 16 KiB, private key 32 KiB, notes and body
  256 KiB, env values and custom values 64 KiB, URLs 4 KiB, tags 64 bytes, file names 1 KiB, MIME types 256
  bytes. 1 MiB stays the ceiling and applies only to `file/<id>/data`.
- **Where limits apply.** A device checks the per-field cap on its own writes and on every op and snapshot it
  reads from another device, because that check is stateless and so is the same on every device. The per-item
  limits (256 fields, 8 files) are checked on a device's own writes only: a count across concurrent edits from
  several devices has no single right answer, and rejecting one device's edit on read would let devices
  disagree about the state.
- **CLI.** `figlock add --type doc --file PATH` (repeatable) and `figlock edit ITEM --file PATH` or
  `--remove-file NAME`; `figlock get ITEM --file NAME --out PATH`. A path is created owner-only (`0600`) and
  never overwritten without `--force`; `--out -` writes to stdout. Plain `get` and `find` show a file's name
  and size, never its contents. Files are read through a size limit and checked before anything is written.
- **Agents.** Unchanged: a document field is reachable by the broker like any other secret field, with the
  usual approval. For example a base64 `.p12` can be injected as an environment variable, which is what CI
  tools expect. Handing a command a file path instead (written `0600` for one command, deleted afterwards) is
  a follow-up with its own approval rules.

## Consequences

- Existing items and vaults are unaffected. Lowering a cap later would make existing data unreadable on read,
  so caps are only ever raised; they are set now, before any release, to avoid that.
- A device on an older build rejects `doc` items from a newer one as unknown fields. Nothing has shipped, so
  this has no effect today. It would need a version gate if it were found after release.
- Importers skip a record whose field is over its cap (`FieldTooLarge`), as they already did for 1 MiB.

## Alternatives considered

- **Native CBOR bytes for file contents.** Exact size, but a second value kind across the Rust API, the
  UniFFI and wasm bindings and every client, plus new test vectors, for no real gain at this size.
- **Raise or remove the 1 MiB ceiling.** Rejected: every device downloads and re-encrypts the whole op log,
  and a large file inside it hurts every sync.
- **Separate encrypted blobs for files now.** Right for large files, too much for keys and certificates. Left
  for a later ADR if the need appears.
- **Enforce the per-item limits on read.** Rejected, see above.
