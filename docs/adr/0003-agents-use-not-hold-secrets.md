# ADR-0003: Agents never see raw values unless the user opts in per item

- Status: Accepted
- Date: 2026-11-03
- Decision: D3 in `docs/decisions.md`

## Context

Secrets pasted into agent chats end up in transcripts and provider logs, which is a core problem. An agent with shell access can always print an injected variable, so there are limits to what is enforceable (`threat-model.md` G5).

## Decision

The default is the broker HTTP mode: the broker adds the auth header itself for the token's allowed hosts, so the agent never receives the value. Env injection with output scrubbing is the second mode. `figlock reveal` is opt-in per item and needs its own approval every time, in loud wording.

## Consequences

- Users get an honest safety statement: broker HTTP holds up against active exfiltration, env injection stops accidental leaks only.
- Every grant is approved with biometrics, time-boxed and written to an encrypted audit log.
- Approval fatigue is a real risk and is handled in the UI design (`design/DESIGN.md` 8).

## Alternatives considered

- Always let agents read secrets: defeats the product.
- Never allow reveal: breaks legitimate use (copying a token to another tool).
- A sandbox that contains a hostile agent: out of scope, not achievable by a password manager.
