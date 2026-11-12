# ADR-0010: Google Drive access limited to the drive.appdata scope

- Status: Accepted (amended by ADR-0026: the local copy is always the primary location and Drive is an optional synced remote)
- Date: 2026-11-03
- Decision: D10 in `docs/decisions.md`

## Context

The `drive.appdata` scope is non-sensitive: the app gets a hidden folder and no access to the user's files, and Google's production verification needs only basic brand checks, no restricted-scope security assessment.

## Decision

The Google Drive adapter requests only `drive.appdata`. Data lives in the hidden appDataFolder, counts against the user's quota and can be removed from Drive settings.

## Consequences

- Users see a small, narrow consent screen, which builds trust.
- The hidden folder belongs to the Google Cloud project, so every platform client must share one project (FIG-40).
- Disconnecting the app can delete the folder, so a full local copy on every device is required (PRD risk table).

## Alternatives considered

- `drive.file` or full Drive: broader access than needed and heavier verification.
- A user-visible folder: easier to delete or tamper with by accident.
