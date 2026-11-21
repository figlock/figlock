# ADR-0039: One prompt per request, no prompt for a request that is gone, and an approved grant is reused

- Status: Accepted
- Date: 2026-11-21
- Decision: D39 in `docs/decisions.md`
- Amends: ADR-0019

## Context

Testing the agent flow (FIG-146) found three ways the broker asks more than it should:

1. Prompts are shown one at a time but never merged. Three identical concurrent requests from one program
   gave three Touch ID windows in a row.
2. Nothing checks that the asking client is still connected. The `figlock` command gives up after 120 s, so a
   queued or retried request still opens a prompt, and approving it issues a grant whose token nobody holds.
3. A grant is never found again. `figlock http` and `figlock run` send a new `grant.request` on every call and
   drop the token, so a second call inside the 15 minute window prompts again. The ttl limited how long an
   unused grant existed, not how often the person was asked.

## Decision

1. **A request that waited behind another prompt is decided again once it reaches the front** (rule 3 below
   or, if the person just denied the same thing, `denied`). So identical concurrent requests cost one prompt.
2. **Liveness.** The server tells the broker whether the client is still connected. A request whose client is
   gone is dropped before its prompt opens, and a grant is not issued if the client left while the person was
   deciding. Neither writes `grant.approved`.
3. **Reuse.** A `grant.request` that an active grant already covers is answered with a new token for that
   grant, with no prompt. A grant covers a request when all of these hold:
   - the same verified identity: user id, program path, and a non-empty chain of parent program paths that
     is equal (pids are not compared; they change on every `figlock` call). A peer without a program path, or
     without parents, is never matched, so the prompt is shown;
   - the same scope (the same item), and the same mode, which is `http` or `run` (never `reveal`);
   - the grant's ttl is `15m`, `1d` or `until-lock` and at least as long as the requested ttl (a request for
     `once` or `session` always prompts, and so does a `1d` request against a `15m` grant);
   - the grant is still live.

   The new token belongs to the same grant: it ends with it, `grant.revoke` ends every token of it, and the
   expiry is never extended. A grant holds at most 32 tokens; the oldest is dropped (a use follows its request
   within moments). `grant.list` still shows one grant.
4. **Audit.** A reused request writes `grant.requested` with the `grant` member set to the covering grant's
   id, and no `grant.approved` (nobody was asked). A normal request has no `grant` member in
   `grant.requested`. No new event name, so older readers still verify the log.

## Consequences

- Inside an approved window the agent is not asked again, as the ttl promised.
- Approval is bound to the verified program chain, not the pid. Another agent (a different chain) still
  prompts. A same-user process that reproduces the chain could ask without a prompt, which G12 already
  accepts: a token is bound to the user, and the same-user attacker could use the agent's own token.
- Chains are best effort (three ancestors, display grade until now). They are used as an identity here only
  to skip a prompt, so any uncertainty (missing path, no parents) fails towards asking.
- `MAX_GRANTS` is no longer used up by an agent that calls the command often.
- A client must keep its sending side open until it has its answer: the broker reads an end of stream as
  "gone". The `figlock` command does.
- Residual: if the client leaves while a system Touch ID window is open, the window cannot be withdrawn; its
  answer is discarded.

## Alternatives considered

- Keep the token in a file for the CLI to reuse: a token on disk is a secret value that outlives the process
  and breaks "the broker keeps only the hash".
- Match on pid: the CLI is a new process each call, so nothing would ever match.
- Match on program only (no parents): through `figlock` the peer is always `figlock`, so any agent would share
  another's approval.
- A pending-request table that joins waiters to one prompt: more state than re-deciding at the front of the
  queue, with the same result.
- A new `grant.reused` audit event: breaks older readers of the log for no gain.
