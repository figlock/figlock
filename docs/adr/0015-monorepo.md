# ADR-0015: One monorepo, plus homebrew-tap and .github

- Status: Accepted
- Date: 2026-11-04
- Decision: D15 in `docs/decisions.md`

## Context

A change to the core touches the CLI, FFI bindings and every app. Separate repos would force version bumps across many repos and split issues and the audit scope (`research.md` 9).

## Decision

Keep core, CLI, apps, relay and the figlock.com site in one repo (`seemantshekhar43/figlock`, later the org). Use path-filtered CI so unrelated jobs do not run. Separate repos only where tooling requires them: `homebrew-tap` and `.github`.

## Consequences

- One PR, one review and one CI run per cross-cutting change, and one commit hash to audit.
- CI must be filtered by path (done in `.github/workflows/ci.yml`).
- Several toolchains in one checkout; mise and just keep that manageable.

## Alternatives considered

- A repo per app: version skew between core and apps, scattered issues, wider audit scope.
