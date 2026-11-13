# ADR-0030: The unlocked desktop session holds the Secret Key

- Status: Proposed
- Date: 2026-11-13
- Decision: D30 in `docs/decisions.md`

## Context

Revoking a device, changing the master password, rotating the Secret Key, setting up the Recovery Key and
adding a device all rewrap the vault key, so they need the master password and the Secret Key
(`Vault::revoke_device`, `change_credentials` and friends take both, ADR-0018, ADR-0022). The desktop app asks
for both again on each of these steps "instead of trusting an open session" (`desk/devices.rs`).

ADR-0027 makes every device keep the Secret Key in its device store, so on a normal device the second step
costs a Touch ID tap or the device password and nothing is typed. The typed case remains for a device that
does not keep the key: the person left "Unlock this Mac with Touch ID" off, or the platform has no usable
store. There the person typed the Secret Key from the Emergency Kit to unlock (or it arrived in the pairing
answer when the device joined), and a minute later the same unlocked window asks them to type it again to
revoke a device. That is the worst moment for it: revoking a lost or stolen device is when the Emergency Kit
is least to hand.

Found during the desktop plus Drive check of FIG-96 and FIG-97 (FIG-107).

What the core needs shows what the session has to keep. Caching the Account Unlock Key (`AUK`) is not enough:

- Changing the password derives the new `AUK` from the new password and the *old* Secret Key.
- Rotating the Secret Key derives it from the *old* password and the new Secret Key, with a fresh Argon2 salt,
  so the password itself is needed, not a derived value.
- A revocation that also changes the password needs the Secret Key and the new password.

So a kept Secret Key plus a typed master password covers every one of these. A kept `AUK` alone covers only a
plain revocation.

## Decision

- **While the desktop vault is unlocked, the window holds the Secret Key it unlocked with, if this device does
  not keep one.** It is set when a full unlock, a create, an open or a join succeeded with a Secret Key that was
  typed or came with the pairing answer, and replaced when the key rotates. It lives in the same zeroizing
  type as the vault key and is dropped with the session on lock, auto-lock, sleep and exit.
- **Steps that rewrap the vault key take the Secret Key in this order**: typed in the window, held by the
  session, kept by the device store (after Touch ID or the device password), and only then asked for. The
  window shows the Secret Key field for these steps only when none of the three exists.
- **The master password is still asked on each of these steps.** It is memorized, not on paper, it is
  required by the derivation, and asking for it keeps the person at the keyboard for an action that locks
  other devices out. The Secret Key is the part people cannot recite.
- **Nothing is cached beyond the session**, nothing is written to disk, and the CLI is unchanged: each
  command is its own process and unlocks with what it is given (or what the device keeps).
- **The window reports it** as `secretKeyHeld` in the vault state, next to `secretKeyKept`. The Settings text
  "Not kept on this Mac" stays true: holding is for the session, keeping is across restarts.

## Consequences

- No new primitive, no format change, no new test vectors. The core API is untouched.
- Threat model: the unlocked process already holds `VK` and every item (`threat-model.md` D6 does not defend
  malware reading an unlocked FigLock process). The Secret Key is one more 256-bit secret in that same memory,
  for the same time, and it is dropped when the vault locks. What it adds against someone who dumps memory
  while unlocked is that they also learn the Secret Key. That is the case where they already have every item,
  and the key cannot be used without the master password.
- An unattended unlocked Mac can now start a revocation with only the master password, where before it also
  needed the Secret Key. Auto-lock (D2) is the defence for an unattended device, and the master password is
  still required. A person who wants the stricter behaviour keeps Touch ID on, which prompts at each step.
- Devices that keep the key are unchanged: Touch ID or the device password is asked at each step, as before.
- Tests: the key is held after a typed unlock, a create, an open and a join; it is dropped on lock and
  replaced on rotation; a step with no held and no kept key still asks; a wrong password with a held key is a
  plain mismatch.

## Alternatives considered

- Cache the `AUK`: not enough, see Context.
- Cache the password too, so a step needs nothing typed: removes the check that the person at the keyboard is
  the owner, and keeps the secret people memorize in memory for the whole session.
- Keep asking: safest on paper, but it trains people to keep the Secret Key in a text file next to the
  password, which weakens the design more than a wiped in-memory copy does.
- Hold the key in the CLI broker so CLI commands can reuse it: out of scope here. The broker has no revoke
  path, and one-shot commands are expected to get their credentials from the device store.
