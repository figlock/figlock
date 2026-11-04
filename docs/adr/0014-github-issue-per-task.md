# ADR-0014: Every task gets a GitHub issue when work starts

- Status: Accepted
- Date: 2026-11-04
- Decision: D14 in `docs/decisions.md`

## Context

Paperclip does not sync with GitHub Issues, but public contributors and PRs need an issue to point to, and GitHub is the public record of code.

## Decision

When a task that changes code moves to In Progress, open a GitHub issue titled `FIG-<n>: <task title>`, comment the link on the Paperclip task, branch `FIG-<n>-<slug>`, and open a PR that says `Closes #<issue>`. Tasks with no code change stay in Paperclip only.

## Consequences

- A small amount of duplicated bookkeeping, handled by the start-a-task routine in AGENTS.md.
- Public history links commits, PRs and issues to task ids.
- Titles and bodies must not contain secrets or local paths.

## Alternatives considered

- Paperclip only: contributors cannot see or reference tasks.
- GitHub only: loses blockers and project documents.
