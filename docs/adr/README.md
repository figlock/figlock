# Architecture decision records

One file per decision that changes direction. `docs/decisions.md` is the index (D1 to D23 map to ADR 0001
to 0023). A change to the crypto or sync spec also needs an ADR and new test vectors before code
(`AGENTS.md`).

## Process

1. Copy the template below into `docs/adr/NNNN-short-slug.md` with the next number.
2. Open a PR titled `FIG-<n> ...`. Status starts as `Proposed` and becomes `Accepted` when the owner approves.
3. Add a row to `docs/decisions.md` with a link.
4. To reverse a decision, write a new ADR and mark the old one `Superseded by ADR-NNNN`. Never delete or
   rewrite an accepted ADR except to fix typos or add the superseded marker.

## Template

```markdown
# ADR-NNNN: Title

- Status: Proposed | Accepted | Superseded by ADR-NNNN
- Date: YYYY-MM-DD
- Decision: D<n> in `docs/decisions.md`

## Context
What forces the decision: facts, constraints, links to `docs/research.md`.

## Decision
What we will do, in one short paragraph.

## Consequences
What gets easier, what gets harder, what we must now do or watch.

## Alternatives considered
Each rejected option and why.
```
