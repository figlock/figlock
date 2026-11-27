# ADR-0050: The broker locks itself when nobody approved anything

- Status: Accepted
- Date: 2026-11-28
- Decision: D50 in `docs/decisions.md`
- Amends: ADR-0040 (the idle lock), `broker.md` 1 and 3.6
- Task: FIG-291

## Context

ADR-0040 made a background broker lock after 30 minutes "with no request and no live grant". That left three holes:

- A broker started with `figlock broker start` in a terminal never locked for idleness, so a forgotten one kept the
  vault key in memory for days.
- Any request restarted the clock, including `status`, so anything that can reach the socket (a monitoring loop, a
  stray process of the same user) kept an abandoned broker open indefinitely.
- A live grant counted as activity, and a grant that lasts "until lock" never ends by itself, so it kept the broker
  open for as long as it existed, which is the situation the timeout is meant to end.

The desktop app already locks when the Mac sleeps or the screen locks (ADR-0030); the CLI broker did not.

## Decision

1. Every broker locks after an idle timeout, on by default at 30 minutes. `figlock broker start --idle-timeout
   <minutes>` (or `FIGLOCK_BROKER_IDLE_MINUTES`, which the background broker started for `http` and `run` inherits)
   sets it; `0` turns it off, with a warning on start.
2. Only approved activity restarts the clock: a grant the person approved, and a request that carries a live
   grant's token and succeeds. Status, hello, malformed requests, denied requests and unknown tokens do not. A live
   grant is no longer activity by itself: when the timeout passes the broker locks, which ends every grant and
   forgets the vault, so a grant can end earlier than its own time limit.
3. The lock is recorded as `broker.lock` with reason `idle`, before the vault is forgotten.
4. `status` shows `idle_timeout_secs` and `idle_left_secs` (null when there is no timeout), and the CLI prints the
   time left.
5. On macOS the broker also locks, with reason `sleep`, when the Mac is about to sleep, the display sleeps, the
   screen locks or the user session is switched away, using the same notifications as the desktop app. A daemon has
   no event loop, so the accept loop serves them from its main thread (`power::pump`, every 500 ms at most).

## Consequences

- An agent that used a long grant and went quiet needs a new approval after 30 minutes. That is the intent.
- The check runs in the accept loop (`Dispatch::tick`), so there is no second timer thread, and tests drive it with
  the fake clock.
- `FIGLOCK_BROKER_IDLE_SECS` can only shorten the timeout and exists for tests.
