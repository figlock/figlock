# ADR-0026: Remembering the Secret Key on a device (opt-in)

- Status: Proposed
- Date: 2026-11-09
- Decision: D26 in `docs/decisions.md`

## Context

A full unlock needs the master password and the 128-bit Secret Key (`crypto.md` 5). `crypto.md` 4 says a device
holds the Secret Key only wrapped by quick unlock, so whenever quick unlock is off, expired (a restart, 4 hours
idle, 7 days), or cannot show a prompt, the person types the Secret Key from their Emergency Kit. That is the
clunkiest step in daily use (FIG-79). Many people would trade a little protection on their own machine for
convenience, as long as they are told what they give up.

What the Secret Key is for matters here: it stops a stolen copy of the vault (a storage breach, a leaked backup)
from being brute-forced when the master password is weak (`crypto.md` 1, threat S2). It is not meant to stop
someone who is already on the person's own unlocked device.

## Decision

- **Opt-in, asked once, never the default.** At first setup (`figlock init` after the Emergency Kit check, and
  when a device joins by pairing) the CLI asks: remember the Secret Key on this device? The answer defaults to
  no. With no terminal (scripts, CI) it never asks and never remembers; `--remember-secret-key` and
  `--no-remember-secret-key` answer it in advance. The apps ask the same question at first sign-in, in the same
  words.
- **The disclaimer is shown before the choice**, in plain words: what stays protected (a stolen copy of the
  vault from storage cannot be opened without the Secret Key, because it never leaves this device) and what is
  weaker (someone who gets into this device while it is unlocked, or a thief with both the files and the login
  session, then needs only the master password). It also says the Emergency Kit is still needed for new devices
  and for recovery, and that choosing no means typing the Secret Key every time a full unlock is needed.
- **Where it is kept.** Wrapped with the same hardware-bound approach as quick unlock (ADR-0025), with its own
  chip key (`secret-key.qk`) and its own wrapped blob (`secret-key.blob`) in the FigLock home, so it does not
  depend on quick unlock being on. The wrap is the ADR-0025 construction with a different purpose: AAD
  context `secret-key` and HKDF info `figlock secret-key se v1`, so a quick-unlock blob can never be opened
  as a Secret Key blob or the reverse. Reading it needs the same user presence (Touch ID, device password or
  Apple Watch) every time. Where there is no Secure Enclave the Secret Key goes in the login keychain behind
  the system prompt, as quick unlock does. It is never in `device.json`, `config.json`, logs or output.
- **What it buys.** A full unlock then needs the master password plus one presence check, instead of the
  master password plus typing the Secret Key. Where no prompt can show (SSH, headless), the check cannot run
  and the Secret Key is asked for as before; that is stated in the disclaimer and in `secret-key status`.
  `FIGLOCK_SECRET_KEY` still wins when set.
- **Changing the choice later.** `figlock secret-key remember`, `forget` and `status` (the apps get the same
  toggle). It is removed when: the person forgets it; the device is revoked; the Secret Key is rotated (a
  remembered key that no longer opens the vault is deleted on the failed unlock and the person is asked for the
  new one); and when the vault is removed from the device. `figlock lock` and `quick-unlock disable` do not
  remove it, because they are about the unlock key, not the Secret Key.
- **Vectors and code.** The wrap gains a purpose parameter in `figlock-core`, with vectors for the
  `secret-key` purpose and for the two purposes failing to open each other. No format of the vault, the op log
  or `vault.json` changes. `crypto.md` 4 and 8 and the threat model (a new row next to S2 and D7) are updated.
- **Other platforms.** Linux and others get the question only once their keystore adapter exists; until then
  the Secret Key stays typed and `secret-key remember` says why it is unavailable. CLI and app stand alone and
  share the file format, as in ADR-0025.

## Consequences

- Daily friction drops for people who opt in: after a restart the next unlock is the master password and a
  touch, not a trip to the Emergency Kit.
- The protection of the Secret Key against a thief with the user's session is reduced on devices that opt in;
  the protection against a stolen vault copy is not. The Secure Enclave wrap and the per-use prompt keep silent
  reading by malware out where the chip exists. Where only the keychain exists, a process running as the user
  can read it behind the system prompt only.
- One more thing to keep in sync on rotation and revocation; the failed-unlock cleanup covers a rotation done on
  another device.

## Alternatives considered

- **Keep the Secret Key in a plain keychain item with no prompt.** Smoothest, and it would work over SSH, but any
  process running as the user could read it silently. Rejected as the default; the per-use presence check is
  the point.
- **Store it in the vault.** Circular: the vault cannot be opened without it.
- **Wrap it with the quick-unlock key.** It would disappear whenever quick unlock expires, which is exactly when
  it is needed.
- **Always require typing it (today).** Safest, and the cost is the friction this task removes.
