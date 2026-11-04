# ADR-0011: Project tracker: Paperclip, run locally with telemetry off

- Status: Accepted
- Date: 2026-11-04
- Decision: D11 in `docs/decisions.md`

## Context

Work is planned across phases, with real blockers and agent-assisted execution. Multica and Paperclip were compared on running locally, licensing, phases, blockers, documents and PR linking (`research.md` 8).

## Decision

Use Paperclip run locally in loopback mode with telemetry off. It holds the goal, projects per phase, tasks, blockers and the project handbook. Neither tool syncs with GitHub Issues, so each task that changes code also gets a GitHub issue when it starts (ADR-0014).

## Consequences

- Planning data is on the owner's machine, backed up with `paperclipai db:backup`.
- PR linking is manual through task ids.
- A second maintainer would need a shared tracker or a migration.

## Alternatives considered

- Multica: needs Docker and Postgres, and has weaker blockers.
- GitHub Projects only: no first-class blocked-by links or documents.
