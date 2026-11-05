# ADR-0019: Broker protocol, grants and audit log

- Status: Proposed
- Date: 2026-11-05
- Decision: D19 in `docs/decisions.md`

## Context

ADR-0003 says agents use secrets and do not hold them, and the PRD wants a broker daemon with approved,
time-boxed grants and an encrypted audit log. The threat model (G1 to G11) names the mitigations but not the
mechanics: how the socket is protected, what an agent sends, what a grant is, and how the audit log is
encrypted and signed. The crypto spec left one question open: whether the audit log gets its own key so the
broker can append without the Vault Key.

## Decision

`docs/spec/broker.md` defines the broker. The points that matter:

- A per-user foreground daemon holds the unlocked vault. Clients talk to it over a Unix socket (directory
  `0700`, socket `0600`, peer uid checked) using one-line JSON requests with strict parsing and small limits.
  Windows is deferred.
- Identity is the verified `{uid, pid, exe}`; a claimed label is shown but never trusted.
- A grant is who, what (item, tag or vault), mode (`http`, `run`, `reveal`) and ttl (`once`, `15m`, `session`,
  `1d`, `until-lock`). The token is 32 random bytes, returned once; the broker keeps only its SHA-256.
  `reveal` is always one item and `once`. Time uses a monotonic clock.
- Approval sits behind a trait (biometric prompt is FIG-35). A terminal yes/no stands in until then, and a
  broker without a terminal denies everything.
- One `authorize` check serves every later mode and enforces locked, token, expiry, mode, scope and the host
  allow-list, and consumes `once` grants atomically.
- The audit log is a local file of records in the existing file container with a new kind `0x03`, sealed under a
  new key `AK = HKDF(VK, "figlock/v1/audit-key")` with AAD context `"audit"`, signed by the device key,
  plaintext canonical JSON, and hash-chained. It is written before the effect, and the effect is refused if
  the write fails.
- `AK` derives from `VK` rather than being a separate append-only key: the broker holds `VK` to open items, so
  a second key would protect nothing.

## Consequences

- `crypto.md` gains `AK` (6) and audit records (7.5); `sync.md` 4 gains kind `0x03`. New vectors
  (`vectors/audit.json`) pin the key, the record bytes and the chain rules, and the Python reference and the
  Rust core must agree before this merges. Existing vectors do not change.
- An agent running as the user can still delete the audit file or truncate its tail; modification and
  forgery are detected. This is stated in the threat model (G10) and in `broker.md` 5, and a signed anchor of
  the log head is listed as an open question.
- A grant token is bound to the user, not to a process. Short TTLs, `http` as the default mode, host
  allow-lists and the audit log are the controls (G12).
- A key rotation changes `AK`; old records are read with the key of their epoch.
- Only macOS and Linux get a broker in this version.

## Alternatives considered

- A separate append-only audit key: no benefit, the broker needs `VK` anyway.
- Audit entries as vault items: they would sync, appear in search and export, and add op files to storage for
  every agent call.
- Plain text or hash-only audit logs: an agent could read or rewrite them.
- Binding tokens to the requesting pid: breaks agents that spawn helper processes, and does not stop a
  same-user attacker, who can act as that process.
- MCP transport instead of a socket: ADR-0008 keeps MCP as an optional adapter.
