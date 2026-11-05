# FigLock broker: socket, grants and audit log

Status: draft v1 (FIG-34). Read with `threat-model.md` 4.3 (G1 to G10), `crypto.md` 6 and 7.5, and
ADR-0003 (agents use secrets, they do not hold them) and ADR-0019 (this document's decisions). It specifies
the local broker daemon: who may talk to it, how an agent asks for access, how access is granted, limited
and ended, and how every decision is recorded. What the broker does with a grant (adding an auth header,
injecting environment variables) is specified with those features: `figlock http` (FIG-37), `figlock run`
(FIG-36) and the biometric prompt (FIG-35).

MUST, SHOULD and MAY are used as in RFC 2119.

## 1. Model

```
 agent process (same user) ──Unix socket, JSON lines──►  broker daemon  ──► unlocked vault (in memory)
                                                              │
                                      approval prompt ◄───────┤  (trait; Touch ID in FIG-35)
                                                              │
                                      audit log  ◄────────────┘  (encrypted, signed, hash-chained)
```

- The broker is a per-user foreground process, started with `figlock broker start`. It unlocks the vault
  once (password and Secret Key, as for any command), keeps it in memory, and forgets it on `lock`, on
  `SIGTERM` or when it exits. It never writes the Vault Key, the password or any item value to disk.
- An agent never receives the vault key. It receives a **grant token**: an opaque handle that lets the broker
  act for it within limits. A token is not a secret value; revealing a value is a separate mode (3.2).
- There is one broker per user and vault. A second `broker start` fails if a live broker answers on the
  socket.

## 2. Socket and peer authentication

Unix only in this version (macOS, Linux). On Windows the broker does not start and says so; a named-pipe
transport with the same protocol is an open question (section 9).

- **Location.** `$XDG_RUNTIME_DIR/figlock/broker.sock`, else `<FIGLOCK_HOME>/run/broker.sock`. The directory
  MUST be owned by the user and mode `0700`; the broker creates it so, tightens it if it exists, and refuses
  to start if it is not owned by the user or cannot be made `0700`. The socket file MUST be mode `0600`
  (created under a `umask` that cannot leave it wider, then checked). A stale socket is removed only after a
  connect attempt fails with "connection refused".
- **Peer credentials.** For every accepted connection the broker reads the peer's user id (and process id
  where the OS gives it: `SO_PEERCRED` on Linux, `getpeereid` and `LOCAL_PEERPID` on macOS). A peer whose uid
  differs from the broker's is closed at once, with no response, and recorded (`conn.refused`). This is in
  addition to the directory and file modes, not instead of them.
- **Identity.** The identity of an agent is `{uid, pid, exe}`, where `exe` is the program path the OS reports
  for `pid` (absent if it cannot be read). The optional `label` in a request is what the agent *claims*; it
  is shown after the verified identity and in quotes, and never replaces it (G7). A spoofed label changes
  nothing about the identity, and the audit log records both.
- **Limits.** One request line is at most 16 KiB. A connection is closed after 30 s with no complete line.
  At most 8 connections are served at once; more are closed with `busy`. Input is parsed strictly: an
  unknown member, a wrong type or a second JSON value on a line is `bad_request`.

## 3. Protocol

UTF-8 JSON, one object per line, newline terminated. One request, one response, in order. A connection may
carry several requests.

```
request   {"v":1, "id":"<text, at most 64 bytes>", "op":"<name>", ...members of the op}
success   {"v":1, "id":"<same>", "ok":true, ...members of the op}
failure   {"v":1, "id":"<same>", "ok":false, "error":{"code":"<code>", "message":"<fixed text>"}}
```

Error codes: `bad_request`, `denied` (the user said no, or nobody answered in 60 s), `no_grant` (unknown,
revoked or malformed token), `expired`, `not_allowed` (outside the grant's scope, mode or host allow-list),
`locked`, `busy`, `audit_failed`, `internal`. Messages are fixed strings. They never contain a vault value,
an item title or any text the caller sent.

Ops:

| op | request members | success members |
| --- | --- | --- |
| `hello` | none | `version`, `locked` |
| `status` | none | `locked`, `grants` (count), `audit_seq`, `audit_head` (hex) |
| `grant.request` | `scope`, `mode`, `ttl`, optional `label`, `reason` | `grant` (3.1) |
| `grant.list` | none | `grants`: list of grant summaries (no tokens) |
| `grant.revoke` | `grant` (id) | none |
| `lock` | none | none; the broker then exits |

Use of a grant (`http.request`, `run.*`, `reveal`) is added by FIG-36, FIG-37 and the reveal task. They
authorise through the same check (3.3), so they inherit every rule here.

### 3.1 Grants

A grant says **who** (the verified identity at request time), **what** (`scope`), **how** (`mode`) and **how
long** (`ttl`).

- `scope` is exactly one of `{"item":"<32 hex>"}`, `{"tag":"<tag>"}` or `{"vault":true}`.
- `mode` is `http` (the broker makes the call and adds the secret itself), `run` (the broker injects values
  into a subprocess and scrubs its output) or `reveal` (the value is returned to the caller).
- `ttl` is one of `once`, `15m`, `session`, `1d`, `until-lock`.
  - `once`: the first successful use ends it.
  - `15m`, `1d`: 900 s and 86400 s, measured on a monotonic clock, so changing the system clock does not
    extend a grant.
  - `session`: until the requesting process (`pid`) exits, or 12 hours, whichever comes first.
  - `until-lock`: until the broker locks or exits.
- `reveal` requires `scope` to be one `item` and `ttl` to be `once`; anything else is `bad_request`. A reveal
  therefore needs its own approval every time (G6).
- At most 64 grants are live; more requests get `busy`.

Success response of `grant.request`:

```
"grant": {"id":"<16 hex>", "token":"<64 hex>", "mode":..., "scope":..., "ttl":..., "expires_in":<seconds or null>}
```

The `token` is 32 bytes from the operating system's random source and is returned once. The broker keeps
only `SHA-256(token)`; a presented token is hashed and looked up, so comparison never touches the token itself
and a memory dump of the broker does not list live tokens. `grant.list` shows `id`, mode, scope, remaining
time and the identity, never a token.

A token is bound to the user, not to a process: any process of the same user that holds it can use it until it
ends. That is the same trust as the agent that was given it (an agent is already the user), and is why the
default mode is `http`, why TTLs are short and why every use is logged (G1, G2, G3).

### 3.2 Approval

Before a grant exists the broker asks an **approver** (a trait; the biometric prompt is FIG-35). The approver
receives the verified identity, the claimed label and reason (shown as untrusted text, control characters
removed), the scope in plain words (item titles are looked up by the broker, so the agent cannot choose the
wording of an item), the mode and the duration. `reveal` and `vault` scopes are marked loud. The approver
returns allow or deny, or nothing within 60 s, which is deny.

- Prompts are shown one at a time. After a deny, the same pid gets `busy` for 5 s (approval fatigue, G8).
- A grant is created only after the audit record `grant.approved` was written (section 5, fail closed).
- Until FIG-35 lands, the daemon's approver is a plain yes/no question on the terminal that started the
  broker, with a visible note that it is not biometric. A broker started without a terminal denies everything.

### 3.3 Authorising a use

Every use of a grant goes through one check, `authorize(token, item, mode, host?)`, which returns the grant
or one error:

1. `locked` if the broker is locked.
2. `no_grant` if the token is not live (never issued, revoked, or already consumed by `once`).
3. `expired` if the grant's time or session ended. An expired grant is removed.
4. `not_allowed` if the mode differs, the item is outside the scope (`tag` is matched against the item's
   tags at the time of use), or, for `http`, `host` is not on the item's host allow-list.
5. On success a `once` grant is consumed in the same step, so two racing uses cannot both succeed.

Host allow-list matching: the item's `allowed_hosts/*` fields hold host names. A host matches an entry if,
after ASCII lower-casing, they are equal, or the entry starts with `*.` and the host ends with the rest and has
at least one more label (`*.example.com` matches `api.example.com`, not `example.com`). An item with no
entries allows no host. Ports and schemes are not part of the match; callers MUST match every redirect hop
again (G3). Internationalised names must be given in their `xn--` form.

Every call to `authorize` writes `use.allowed` or `use.refused` before it returns.

## 4. What the broker never does

- It never returns a vault value except in `reveal` mode, and never writes one to the audit log, to its own
  logs, to an error message or to a prompt.
- It never accepts a request from another user, and never creates a grant without a fresh approval.
- It never extends a grant. A new request, and a new approval, is the only way to more access.

## 5. Audit log

One record for every event that matters, written **before** the effect it describes (fail closed: if a
record cannot be written, a grant is not created and a use is refused with `audit_failed`).

- **Where.** `<FIGLOCK_HOME>/audit/<vault_id hex>.log`, mode `0600`, append-only, on this device only. It is
  not synced.
- **Format.** A sequence of records, each `u32 length || container`, where the container is the `sync.md` 4
  file container with kind `0x03`, sealed under the audit key and signed by this device's key
  (`crypto.md` 7.5). `seq` starts at 1 and grows by exactly 1. The plaintext is canonical JSON (RFC 8785):

```
{"d":{...}, "ev":"<event>", "prev":"<64 hex>", "t":<unix ms>, "v":1}
```

  `prev` is the SHA-256 of the previous record's container bytes (64 zeros for `seq` 1).
- **Events.** `broker.start`, `broker.lock`, `conn.refused`, `grant.requested`, `grant.approved`,
  `grant.denied`, `grant.revoked`, `grant.expired`, `use.allowed`, `use.refused`.
- **Detail (`d`).** Only these members, with these types, so a value cannot get in by accident:
  `peer` (`{uid, pid, exe}`), `label`, `reason` (control characters removed, at most 200 characters),
  `grant` (id), `mode`, `ttl`, `scope` (`item:<id>`, `tag:<tag>` or `vault`), `item` (id), `field` (field
  *name*), `host`, `code` (error code). Unknown members are rejected when writing.
- **Reading.** A reader MUST verify, record by record and before decrypting: the container and its signature
  against the device key, `seq` continuity, and the `prev` chain. It reports the first record that fails and
  stops there; it never skips a bad record to show later ones. Bytes after the last whole record (a write cut
  short by a crash, or tampering) are reported. On start the broker drops such an incomplete tail, writes
  `broker.start` after the last good record, and refuses to start if any whole record fails verification
  (the user moves the file aside; the broker never overwrites a log it cannot verify).
- **What this gives.** Anyone without the Vault Key (including an agent running as the user) can read nothing
  from the file, and cannot change, insert, reorder or remove a record in the middle without detection. It
  *cannot* prevent deletion of the file or truncation of its tail, because the agent runs as the same user
  (accepted, G10). `status` and `broker.start` show the current `seq` and head hash so the user can notice.

## 6. Keys and memory

- The broker derives the audit key `AK` from `VK` (`crypto.md` 6) when it unlocks and holds it in a zeroising
  wrapper. It needs `VK` anyway to open items, so there is no separate append-only key.
- Grant tokens exist in memory only as hashes after they are returned. The vault is dropped (zeroised) on
  `lock`, on a signal and on exit. Core dumps are not disabled by this version (open question).

## 7. Commands

`figlock broker start` (foreground), `figlock broker status`, `figlock broker grants`,
`figlock broker revoke <grant id>`, `figlock broker lock`, and `figlock audit [--tail N]` (unlocks the vault,
verifies and prints the log with the same redaction as the log itself).

## 8. Required tests

Unit and end-to-end, all with generated values (`AGENTS.md`):

- Socket mode `0600` and run directory `0700`; start refused when the directory is not private; a stale socket
  is replaced, a live one is not.
- Wrong uid refused without a response and logged (a unit test on the credential check, since a second
  user cannot be created in CI).
- No grant, malformed token, revoked grant: refused. `once` consumed by the first use, including under a race.
  `15m`, `1d`, `session` and `until-lock` end as specified (monotonic clock injected in tests).
- Host not on the allow-list, wildcard rules and an item without entries: refused. Redirect to another host:
  refused.
- `reveal` with any ttl but `once`, or a non-item scope: `bad_request`. Reveal twice needs two approvals.
- Denied, timed out and prompt-spam cases; a spoofed `label` does not change the identity shown.
- Strict parser: oversize line, unknown member, wrong type, two values on a line.
- Audit: a secret value never appears in any record, decrypted or not; ciphertext has no plaintext; a
  flipped bit, a swapped, dropped or inserted middle record and a wrong key are each detected; a grant is not
  created when the log cannot be written.
- Vectors for the audit record container and chain (`vectors/audit.json`).

## 9. Open questions

- Windows transport (named pipe with a security descriptor) and the process chain on Windows (threat model 8).
- Whether the prompt should show the parent process chain, not only `exe` (FIG-35).
- Disabling core dumps and locking pages for the broker process.
- A signed anchor for the audit log head (for example in the vault) so tail truncation can be detected.
- Whether `session` should use a terminal session or process group instead of one pid.
