# ADR-0012: The repository is public from day 1

- Status: Accepted
- Date: 2026-11-04
- Decision: D12 in `docs/decisions.md`

## Context

The product's value rests on being auditable and trusted. Hiding the code until later delays community review and contradicts the non-negotiables around transparency.

## Decision

The repo is public from the first commit. Secrets, local paths and personal data never go in it (AGENTS.md non-negotiables), enforced by secret scanning with push protection.

## Consequences

- Planning, specs and decisions are public. Anything private lives in Paperclip, not in the repo.
- Mistakes (a leaked token) are permanent, so scanning and review rules are strict.
- Early work is visible to everyone, including incomplete specs.

## Alternatives considered

- Private until the audit: loses early review and trust, and delays the open-source story.
