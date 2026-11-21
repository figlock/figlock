# ADR-0040: The desktop app hosts the broker, and approvals start from a notification

- Status: Accepted
- Date: 2026-11-21
- Decision: D40 in `docs/decisions.md`
- Amends: ADR-0019

## Context

The broker (ADR-0019) is a separate CLI daemon. Testing the agent flow (FIG-146, FIG-147) showed what that costs:

- The person must run `figlock broker start` in a terminal before any agent can use a secret. The agent cannot
  do it, and the CLI only prints "no broker is running".
- The daemon unlocks the vault on its own (master password, or Touch ID with quick unlock), even when the
  desktop app is open and already unlocked.
- Approvals are a system Touch ID window with nothing on screen to explain it. The desktop app knows nothing
  about the broker: there is no broker code under `apps/desktop`.

Facts the design rests on:

- The broker lives in the `figlock-cli` binary crate, owns its `Session` (`State.session`) and reaches the
  vault only through it.
- The desktop `Desk` keeps one `Session` in `Inner` behind a mutex, and locks it on auto-lock, sleep and exit.
- The system prompt is the only thing that proves it is the person (Touch ID or the device password). A
  notification, or a button in a notification or in a window, can be clicked or faked by any process of the
  user, so it can never be an approval (G8, G12, `broker.md` 3.2).

## Decision

1. **One host per user and vault, and the desktop app is the preferred host.** While the app is running and
   unlocked it serves the broker socket, using the session it already holds. There is no second unlock and no
   `broker start`. The CLI daemon stays as the headless host (decision 6).
2. **The broker no longer owns the session.** The broker rules move out of the CLI binary into a library
   crate, `figlock-broker`. The broker asks a `SessionHost` for the vault (`with_session`, which answers
   "locked" when there is none) instead of holding a `Session`. The CLI daemon implements the host over its
   own session; the app implements it over `Desk`'s. When the host locks (auto-lock, sleep, quit, a revoked
   device) every grant ends, exactly as the broker locking does today. No change to the socket protocol, the
   grant rules, the audit log or their vectors.
3. **Approvals start from a notification and are decided with the system prompt.** When a request needs a
   prompt and the app hosts the broker:
   1. The app posts a notification: "<verified program> wants <item> for <ttl>". The text comes from
      `prompt_line` (the broker's own words, never the caller's label).
   2. Clicking it, or opening the app, shows an **approval card** with what `describe` shows today: verified
      program and parents, the reason marked unchecked, item, allowed hosts, ttl, loud wording for `reveal`
      and the whole vault.
   3. **Approve runs the system Touch ID or device-password prompt, in the app.** Deny, dismiss, the system
      prompt failing, or 60 s without an answer is deny. A click on a notification or on Approve never
      issues a grant by itself.
   4. One card at a time, with the rest queued. FIG-146 already merges identical requests and drops requests
      whose client has gone, so the card never shows stale or duplicate asks.
4. **Fallbacks, in order.** If the notification cannot be shown (permission off, app window cannot open), the
   broker uses the direct system Touch ID prompt, as today. With no biometric prompt it asks on a terminal, and
   with neither it denies everything and says why (`broker.md` 3.2). The 60 s deny remains the safety net for a
   notification the system hides (Focus mode). A request that arrives while the app is locked is answered
   `locked` with a message that says to unlock FigLock; no notification is sent for a locked vault, so a locked
   Mac shows nothing it cannot act on.
5. **A CLI broker that is already running.** The app does not steal the socket. It shows a banner, "A CLI broker
   is running", with a button to stop it and host here. The button sends the existing `lock` request to that
   broker, which ends its grants and exits, then the app binds the socket. Nothing changes until the person
   clicks. The reverse (`figlock broker start` while the app hosts) fails with a message that the app is
   hosting, using the existing live-broker check.
6. **No desktop app.** The CLI daemon keeps working on its own:
   - If no host answers, `figlock http` and `figlock run` may start one in the background, but only when quick
     unlock is valid, so nobody has to type a password at an agent. The background broker unlocks with one
     Touch ID, then the request gets its normal approval prompt: two prompts on a cold start, none after that
     inside the grant window. It locks itself after 30 minutes with no live grant.
   - If quick unlock is off or expired, a background start would need a master password and nobody to type it.
     The command fails at once with exit code 69 and one line: run `figlock broker start` or open FigLock. It
     does not wait out the 120 s client timeout.
   - With no biometric prompt and no terminal (headless Linux, CI) the broker denies everything and the
     command says so.
   - A request answered `locked` or `no broker` prints a message that names the two ways forward.
7. **Search commands are out of scope.** `find`, `get` and `copy` still unlock from scratch each call. Letting a
   CLI command borrow the running host's session is a separate decision and is not made here.

## Consequences

- An agent works whenever FigLock is open and unlocked, with no terminal step. The first time per window the
  person sees a notification and a card, and approves with Touch ID.
- The broker crate becomes the unit that is tested without a UI: the approver and the session host are traits,
  as the approver already is.
- The app gains a notification plugin, a card, a socket thread and a banner. The app's lock paths must also end
  broker grants, which adds a way for a lock to be missed; one test per lock path (auto-lock, sleep, quit,
  revocation) covers it.
- Two hosts can never serve at once, so a grant never lives in two places. Taking over ends the CLI broker's
  grants, which is why it needs a click.
- A same-user process that can reach the app's window could click Approve, but the click only starts the
  system prompt. Authority stays with Touch ID.
- Notifications can be delayed or hidden by the system. The 60 s deny means an agent waits at most that long
  and the person is never approved by default.

## Alternatives considered

- **Keep the CLI daemon and have the app only show approvals.** Needs a second protocol between app and daemon,
  and the person still starts the daemon and unlocks twice.
- **Let the notification's Approve button approve.** A notification action is not authentication.
- **The app launches `figlock broker start` itself.** Two unlocks and two sessions, and the broker still
  cannot reach what the app has open.
- **Refuse to host while a CLI broker runs.** Simple, but the person must know to stop it. The banner does the
  same safely.
- **One combined Touch ID that unlocks and approves a cold-started CLI broker.** Fewer taps, but it couples
  unlock to a request the broker has not parsed yet. Left out; revisit if the two taps annoy.
- **Auto-launch the app from the CLI.** Starts a GUI, and an unlock, because a command ran. Not done.

## Work, in order

1. Extract `figlock-broker` and the `SessionHost` trait; the CLI daemon uses it (no behaviour change).
2. The app hosts the socket; lock paths end grants; the takeover banner.
3. Notification and approval card; fallback chain.
4. CLI: background start, fail fast with exit code 69, idle lock, updated messages and skill.
