# ADR-0016: The repo lives under the personal account first

- Status: Accepted
- Date: 2026-11-04
- Decision: D16 in `docs/decisions.md`

## Context

An organization adds overhead before there is anything concrete, but the repo should end up under a project owner and user-facing links should not break when it moves.

## Decision

Host at `seemantshekhar43/figlock` now and transfer to a `figlock` organization before the first store or Homebrew release (FIG-14). User-facing links use figlock.com, not github.com paths.

## Consequences

- A GitHub transfer keeps issues, PRs, stars and releases, and old URLs redirect.
- After the move: update the crates.io repository field, the Homebrew tap, Actions secrets, Pages and branch rules.
- Some files already mention the github.com path (for example the issue template contact link) and are updated at transfer time.

## Alternatives considered

- Create the org now: reserved name is optional and empty (FIG-1); the overhead is not needed yet.
