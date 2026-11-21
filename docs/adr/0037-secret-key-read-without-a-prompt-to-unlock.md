# ADR-0037: Unlocking with the master password never raises a second prompt

- Status: Accepted
- Date: 2026-11-21
- Decision: D37 in `docs/decisions.md`
- Amends: ADR-0034

## Context

ADR-0034 kept the Secret Key in the Secure Enclave while quick unlock was on, read after Touch ID or the device
password. A person who typed their master password on the Unlock screen then saw a Touch ID prompt anyway,
because the unlock reads the kept key. Typing the password is a complete unlock; a second biometric step
after it is wrong (FIG-140).

## Decision

1. **The kept Secret Key always lives in the login keychain item shared by our apps (ADR-0031),** whether or
   not quick unlock is on. Unlocking with the master password reads it with no prompt.
2. **Quick unlock still guards the unlock key on the chip.** Touch ID is the way in for "Use Touch ID"; the
   password is the way in for the password. Never both in one unlock.
3. **Deliberate releases of the key still confirm when quick unlock is on:** Show Secret Key, Save Emergency
   Kit and approving a new device ask for Touch ID or the device password first. With quick unlock off they
   read it with no prompt, as before.
4. **Migration:** a key an earlier build left on the chip is read once after the prompt and moved to the
   login keychain, then never prompts on unlock again. A declined prompt leaves it where it was.
5. Turning quick unlock on or off no longer moves the key (`retier` is gone). Nothing in the file format or
   crypto changes.

## Tier trade-off

With quick unlock on, the Secret Key is now as exposed as it already was with quick unlock off (ADR-0034): a
person at an unlocked, logged-in Mac, or malware running as them, can read it from the keychain. It is still
useless without the master password. Showing it and handing it to a new device keep their prompt.

## Alternatives considered

- **Keep the key on the chip and prompt on password unlock:** the reported bug.
- **Hold the key in memory between unlocks:** gone at restart, the case ADR-0027 exists for.
- **Drop the prompt on Show and add device too:** removes a check people rely on for no gain.
