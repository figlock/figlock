# ADR-0002: The user picks the storage; suggest iCloud Drive or Google Drive

- Status: Accepted
- Date: 2026-11-03
- Decision: D2 in `docs/decisions.md`

## Context

There is no FigLock backend (AGENTS.md non-negotiables). Storage must be something the user already owns. Research found Google Drive appDataFolder works everywhere, a synced folder works on desktop and iOS only, Git and S3 are later, and CloudKit locks out Android and Windows.

## Decision

There is no default silently chosen. Setup asks the user to pick, and suggests iCloud Drive (a synced folder) for an iPhone plus Mac only setup, and Google Drive for anything mixed. All adapters implement the same five-operation BlobStore contract (`sync.md` 2).

## Consequences

- Adapters must stay dumb: `get`, `put_new`, `put_replace`, `list`, `delete`. Anything smarter is built above them.
- Support burden grows with each adapter, so only a local or synced folder (phase 1) and Google Drive (phase 3) ship early; Git and S3 are later.
- iCloud Drive is supported as a folder, not through CloudKit.

## Alternatives considered

- A single mandatory provider: simplest, but locks users in and conflicts with the product promise.
- CloudKit: rejected because it excludes Android and Windows.
- Pure peer-to-peer: only workable for pairing, not for always-available sync.
