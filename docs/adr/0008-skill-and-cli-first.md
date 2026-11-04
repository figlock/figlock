# ADR-0008: Agent interface: Claude skill and CLI first; MCP optional

- Status: Accepted
- Date: 2026-11-03
- Decision: D8 in `docs/decisions.md`

## Context

Agents that can run shell commands can use a CLI directly with no extra server, and a skill loads only when needed. MCP adds a server and always-on tool definitions, but shell-less chat clients need something.

## Decision

Ship a `figlock` CLI with `--json` on every command and a Claude skill installed by `figlock skill install`. An MCP adapter is an optional add-on in phase 6 for clients without a shell.

## Consequences

- No always-on server to secure for the main path; the broker is a local socket daemon.
- Shell-less clients (some chat apps) are unsupported until phase 6.
- The skill text is part of the security model (it steers agents to broker HTTP) and is reviewed like code (`threat-model.md` G11).

## Alternatives considered

- MCP first: more moving parts and always-loaded context.
- A custom agent protocol: nothing would speak it.
