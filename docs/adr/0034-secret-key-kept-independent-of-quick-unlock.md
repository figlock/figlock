# ADR-0034: The Secret Key stays on the device whatever quick unlock does, and a counter says when it changed

- Status: Accepted
- Date: 2026-11-15
- Decision: D34 in `docs/decisions.md`
- Amends: ADR-0027

## Context

ADR-0027 says every device keeps the Secret Key, so a restart or a new day asks for the master password only.
The desktop did not hold to that, and it blurred two different things:

- The "Unlock this Mac with Touch ID" choice also decided whether the key was kept. With it off, the setup text
  and Settings said "the key is only kept with it. You type it from your Emergency Kit each time you unlock."
  Quick unlock is a convenience, the Secret Key is what the design relies on; the two must not be linked.
- A wrong master password and a Secret Key rotated on another device look the same to the device: both fail to
  open the vault. The unlock screen answered both by showing the Secret Key fields. A person who mistyped the
  password was asked for a 40 character key they may not have at hand.
- A device that kept its key and a prompt-free read: every read of the kept key asked for Touch ID or the
  device password, even when the person had turned quick unlock off, so "master password only" was not true.

Found in FIG-121 and decided by the maintainer in its review (FIG-132).

## Decision

1. **The key is kept whether or not quick unlock is on.** With quick unlock on it is held in the strongest tier
   the Mac has (the Secure Enclave, read after Touch ID or the device password, ADR-0025). With quick unlock off
   it is a login-keychain item shared by our apps (ADR-0031) and read with no prompt. Turning quick unlock on
   moves the same key up to the chip, turning it off moves it down; neither deletes it or makes a new one.
   "Forget the key on this Mac" in Settings stays the only way to drop it.
2. **A non-secret signed counter `sk_gen` in `vault.json`** (`crypto.md` 9.4). It starts at 1 and is omitted
   while it is 1. Only a new Secret Key raises it: rotation, a revocation that rotates, and recovery with a
   Recovery Key. A new master password, a revocation alone, and the Recovery Key members leave it. A file with
   `sk_gen` is `min_format_version` 2, so a client that predates it refuses the file instead of dropping the
   counter.
3. **Each device stores the `sk_gen` its kept key belongs to**, next to the key (not secret, in the device
   config folder). Rotating on this device updates the key and the number in one step.
4. **Unlock screen.** A wrong password says "That password did not open the vault" and never shows the key
   fields. The key fields show up front only when the vault's `sk_gen` is higher than the kept one (or no key is
   kept at all), with a plain notice before anything is typed: "Your Secret Key was changed on another device.
   Type the new one from that device's Emergency Kit or Show Secret Key." A small "My Secret Key changed" link
   opens the fields by hand, for a rotation made by a client that does not write `sk_gen`. One entry of the new
   key fixes it for good: the key and number are kept and the screen asks for it no more.
5. **A vault older than this device has seen** (`sk_gen` lower than the kept number) is refused as a rolled-back
   `vault.json` (`sync.md` 8). The check is a device-local rule; the file itself stays valid.
6. **A running app** that sees a higher `sk_gen` after a sync shows a banner "Your Secret Key was changed on
   another device" with an Update now button. It asks for the master password and the new key, checks them
   against the vault, keeps them, and the open session keeps working. The CLI says the same on the next command
   that unlocks and in `figlock sync status`.

Not done here, on purpose: the notice does not name the device or the date, because that needs another signed
member and nothing else needs it yet; a system notification needs a notification permission and plugin, so the
banner is the only alert for now.

## Tier trade-off, in plain words

With quick unlock off the Secret Key sits in the login keychain with no Touch ID in front of it. A person at an
unlocked, logged-in Mac, or malware running as that person, can read it. The key is still useless without the
master password (it is the same exposure the stored Account Unlock Key had in ADR-0027), and it removes the
common failure of ADR-0027's weaker alternative, a person who turns Touch ID off and then keeps the key in a
text file. People who want a prompt on every read keep quick unlock on.

## Consequences

- With quick unlock off, a restart and a day later the Mac unlocks with the master password alone.
- A wrong password is told apart from a stale key, so the key fields appear exactly when the key must be typed.
- `crypto.md` 9 and `pairing-recovery.md` 6 get the new rules; `sk-gen.json` holds the vectors, and the Rust
  core and the Python reference both check them.
- Every writer of `vault.json` must carry `sk_gen` through: a client that does not is refused by version.
- A device that never saw the vault since a rotation has a stale key by construction; it asks once.
- The key lives in the login keychain when quick unlock is off, so the threat model's "unattended unlocked Mac"
  row (D6) now includes the Secret Key (still not enough alone).

## Alternatives considered

- **Keep as is** (key tied to quick unlock; fields shown after any failure): rejected by the maintainer.
- **Ask for the key only after repeated failures:** a counter of failures is device state an attacker can reset
  and still shows the key fields to the wrong-password case.
- **Compare a hash of the key to learn staleness:** needs a verifier in a file anyone with the vault can read;
  a counter reveals nothing about the key.
- **Put the generation in the file name or an unsigned file:** a storage attacker could then choose which
  prompt a person sees; the signed counter, and the unlock that follows it, decide.
