# ADR-0027: Each device stores the Secret Key; people see it once

- Status: Accepted
- Date: 2026-11-12
- Decision: D27 in `docs/decisions.md`

## Context

The Secret Key (`crypto.md` 4) is what makes a stolen vault file safe against offline guessing, because FigLock
has no server to count wrong guesses. It also has to be easy to live with: asking people to retype a 40
character key on every new device or after every restart is the main usability cost of the design.

What the code does today differs from what the spec assumes:

- `crypto.md` 4 says the Secret Key lives on a device "only wrapped by quick unlock", and recovery case R4
  (`pairing-recovery.md` 6) says a paired device can show it after a biometric check.
- In the code no device stores it. Quick unlock keeps only the Account Unlock Key (`quick.rs`), and
  `config.rs` says the Secret Key is never written. A restart, or 7 days, needs both secrets typed again.
- Pairing (`pairing-recovery.md` 3) moves the Secret Key to a new device, and `Vault::approve_join` seals it,
  so the trusted device must hold it. It cannot, so the "Add a device" screen cannot be built with only a
  Touch ID tap.

The maintainer weighed four options (keep as is; password only; hide the key behind pairing; cloud login) and
chose to hide the key behind pairing, with a simple user story: remember one master password, keep one
Emergency Kit, tap Touch ID. The page with the reasoning is attached to FIG-100.

## Decision

- **The Secret Key is shown once**, at vault creation, to be saved in the Emergency Kit (with the forced
  re-entry check of `pairing-recovery.md` 5). After that it is visible only through "Show Secret Key" in
  Settings or `figlock secret-key show`, behind user presence, and when it rotates.
- **Every device keeps the Secret Key in its device store.** It is written at vault creation, at a full unlock
  and when a device joins, and read by a full unlock, so a restart or idle expiry asks for the master password
  only. It is deleted when the device is revoked, when the person turns quick unlock off with "forget this
  device", and replaced when the Secret Key rotates.
- **A new device gets the Secret Key by pairing** from a device that holds it. Typing it from the Emergency
  Kit is the fallback when no trusted device is available.
- **Three protection tiers.** Every platform gets at least tier 2. The app and the CLI show which tier a
  device has (as `quick-unlock status` already does for the Mac).
  1. *Hardware plus presence*: a key in a secure chip, released by Touch ID, Face ID, Windows Hello or the
     device password. The Secure Enclave on Apple devices (ADR-0025), the Android Keystore, a TPM with
     Windows Hello where a prompt can be shown.
  2. *OS login store*, the way Chrome protects its own key: the macOS Keychain, Windows DPAPI, the Linux
     Secret Service or KWallet. It needs no window, so the CLI can use it.
  3. *Headless fallback*: an owner-only file, as ssh keys and cloud CLI credentials are kept, labelled as
     weaker. For servers, CI and containers with no login store.
- **The stored Secret Key is never enough on its own.** A thief still needs the master password. The vault
  file on the storage still has no Secret Key in it.
- **Recovery Key at setup is a required choice screen**: add one now, or acknowledge that a forgotten
  master password with no device cannot be recovered (`pairing-recovery.md` 6, R3). The Recovery Key
  construction in `pairing-recovery.md` 7 is unchanged.
- **Approving a pairing from a device that is not nearby is phase 2.** Until then a person with no device at
  hand uses the Emergency Kit.
- **No crypto or format change.** One vault has one master password and one Secret Key on every device. No
  new primitive, no new test vectors. `crypto.md` 4 is updated to match.

## Consequences

- People type the Secret Key only when no trusted device is at hand, and every unlock after a restart needs
  the master password and no more. This is the main usability gain.
- Exposure grows by one item: a device that is stolen and unlocked (tier 1 prompt passed, or tier 2 while
  logged in) yields the Secret Key. It still needs the master password, which is the same exposure the stored
  Account Unlock Key already has. Tier 3 is weaker by design and says so.
- Rotating the Secret Key (FIG-88 slice 5) must update the stored key on every device, or tell each one to
  re-pair or type the new key from the new kit. The "other devices need a full unlock" notice of ADR-0022 stays.
- Needs work: the device-store write and read paths and "Show Secret Key" (FIG-101), the trusted-device side
  of pairing (FIG-102), the Recovery Key (FIG-103), CLI join, open and approve (FIG-104) and the Windows and
  Linux stores (FIG-105). FIG-88 slice 5 waits for FIG-101.
- Open points to settle by small tests before the code: DPAPI and the Secret Service from a console binary
  without a window, and whether Windows Hello can be asked for from a console app. If Hello cannot, Windows
  stays at tier 2 for the CLI.

## Alternatives considered

- **Keep today's behavior** (type both secrets on every new device and every restart): simplest, and the
  safest against a stolen device, but the friction is the reason people drop password managers.
- **Master password only, with a strong Argon2id setting** (Bitwarden style): needs no key at all, but a
  weak or reused password falls to offline guessing of a stolen vault file, and FigLock has no server to
  limit guesses.
- **Cloud login** (Apple and Google style): safe because their server or hardware counts guesses, but needs a
  backend and an account system, which AGENTS.md rules out.
- **Keep the Secret Key in the OS cloud keychain** (iCloud Keychain, Android Block Store), so a new device of
  the same ecosystem needs no pairing: rejected. It works only inside Apple or Android, not on Windows or
  Linux, so pairing is needed anyway, and it moves trust to the OS account.
