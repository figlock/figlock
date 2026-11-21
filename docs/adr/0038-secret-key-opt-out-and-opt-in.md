# ADR-0038: Not keeping the Secret Key is a remembered choice, and it can be undone

- Status: Accepted
- Date: 2026-11-21
- Decision: D38 in `docs/decisions.md`
- Amends: ADR-0034

## Context

Settings offered "Stop keeping it on this Mac" and nothing to undo it (FIG-142). Worse, the choice did not
hold: every full unlock keeps the key it was opened with, so the next unlock with the key typed quietly put it
back. And the key field showed whenever a Mac kept no key, whatever the reason, so there was no rule for when
a person may be asked to type 40 characters.

## Decision

1. **Stopping is a remembered, per-vault choice** in the device config folder (`secret-key-opt-out.json`, not
   secret). While it is set, `SecretKeyStore::put` keeps nothing: not a full unlock, not an update banner, not
   a rotation. Every full unlock asks for the key until the person keeps it again.
2. **Keeping it again is one action** (Keep it on this Mac in Settings, `figlock secret-key keep` in the CLI). An unlocked
   desktop window already holds the key it unlocked with (ADR-0030), so it asks for nothing; a window that does
   not hold it asks for the master password and Secret Key once, checks them against the vault, and keeps the
   key. The choice is cleared in the same step. After that the master password is all that is typed.
3. **When a person types the Secret Key.** Only when (a) they stopped keeping it, (b) another device rotated it
   (`sk_gen`, ADR-0034), (c) this Mac has no secure place to keep it, or (d) the vault was just opened here and
   no key has been kept yet, once. Never because Touch ID was turned on or off (ADR-0037).
4. The window's state says which case it is (`secretKeyOptedOut`), so the Unlock screen and Settings say why
   instead of guessing. `figlock secret-key forget` sets the same choice.

No crypto or file format change.

## Alternatives considered

- **Treat any Mac with no key as opted out:** a Mac that never had one yet would never keep one.
- **Clear the choice at the next full unlock with the key typed:** the bug being fixed.
- **Ask for the master password again on keep when the window holds the key:** it was checked at unlock and the
  key never leaves the process; a second ask adds friction and no safety. The held key is still refused, and the key asked for,
  when the vault's `sk_gen` moved on since this window unlocked.
