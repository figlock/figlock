# ADR-0047: Sync triggers - push on write, sleep when idle

- Status: Accepted
- Date: 2026-11-27
- Decision: D47 in `docs/decisions.md`
- Amends: ADR-0026 (schedule), `sync.md` 10
- Task: FIG-261

## Context

Three things matter for sync on every device: it must not lag behind other devices, a local write must reach the
remotes at once, and it must not drain the battery. The state before this ADR:

- Desktop pushed a save within a second (a dirty flag read by a thread that woke every second), but the thread woke
  sixty times a minute even when idle. The broker daemon's accept loop woke twenty times a second for as long as it ran.
- iOS pushed nothing after a save. An edit showed "Not synced yet" until the next turn of the 15 second foreground
  loop, and an edit made just before leaving the app waited for the system's background refresh.
- The CLI already pulls when the last sync is over a minute old and pushes after a write.

## Decision

1. **A write is pushed at once.** On iOS, every save, delete and restore asks `PushScheduler` to run a sync with
   `force` (a person acted, so the error back-off does not apply). The first write runs immediately. Writes that arrive
   during a run, or within 2 seconds after it, share one more run. Nothing runs when nothing was written.
2. **A push is not cut off.** The run holds a UIKit background task, and the app waits for a running push as it goes to
   the background, so a save made just before switching away still reaches the remotes within the time iOS grants.
3. **Idle means asleep.** The desktop sync thread sleeps until the nearest deadline (a place due, the open vault's next
   look at the local folder, at most one interval, at least 250 ms) or until a save wakes it, instead of waking every
   second. The broker's accept loop waits in `poll` (at most 500 ms, so it still notices the broker finished) instead of
   napping 50 ms.
4. **The 15 second poll and the 5 s / 15 s / 60 s / 5 min back-off stay.** They are the backstop for changes made by
   other devices, which no event reaches on a plain folder or Drive.
5. **The CLI keeps its on-demand model.** Pull before a command when stale (over a minute), push after a write. A
   one-shot command never loops or leaves a process behind; only the broker daemon runs the loop.

## Not decided here

Event-driven pulls (a file watcher on the vault folder and the iCloud folder) and a Drive changes feed to cut a no-op
Drive sync from about 3 seconds are separate tasks. They lower latency and battery further but need a watcher
dependency and a change to the Drive adapter, which this change does not.

## Consequences

- A save on a phone is on the remotes in seconds, with the app in front or just left.
- An idle Mac app and a running broker use far fewer wake-ups.
- A burst of edits costs one sync, then at most one more, not one per edit.
