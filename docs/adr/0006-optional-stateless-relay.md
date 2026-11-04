# ADR-0006: Optional stateless relay for phone approvals

- Status: Accepted
- Date: 2026-11-03
- Decision: D6 in `docs/decisions.md`

## Context

Approving an agent request from the phone is valuable, but a laptop and phone are not always on the same network and push needs a relay. The no-server rule forbids anything that stores vault data (AGENTS.md non-negotiables).

## Decision

Provide a Cloudflare Worker relay at relay.figlock.com that forwards end-to-end encrypted approvals (and push via APNs) and stores nothing. Everything works without it; with it, approvals are signed by the approving device (`threat-model.md` G9).

## Consequences

- The relay sees only ciphertext, sizes and timing; it cannot mint approvals.
- It is optional, so users can run without any hosted component, or self-host it later.
- It is a new operated component, so its uptime and abuse handling become our responsibility once phase 5 ships.

## Alternatives considered

- No relay: phone approval only on the same network or via the cloud storage with long latency.
- A stateful backend: breaks the no-backend rule.
