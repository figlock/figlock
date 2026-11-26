# ADR-0045: The Recovery Key is the single code, SLIP-0039 and Shamir cards are removed

- Status: Accepted
- Date: 2026-11-26
- Decision: D45 in `docs/decisions.md`
- Supersedes: ADR-0028 (SLIP-0039 in `figlock-core`)
- Amends: `pairing-recovery.md` 7 (Recovery Key), `crypto.md` 4.2 and 9.3, `threat-model.md` R1 and R3
- Task: FIG-242. Follow-up: FIG-243 (a standards-based alternative)

## Context

The optional Recovery Key could be written down as one `FLR1-` code or split with SLIP-0039 into `M` of `N` cards
(ADR-0028). Only the desktop app offered the cards; the iPhone only ever took the single code.

For the iPhone's first TestFlight upload (FIG-175) the app's cryptography has to be declared to Apple and, under
the US export rules, classified (`docs/ios-export-compliance.md`). Everything FigLock uses is a standard primitive
from the IETF, NIST or IEEE (Argon2id, HKDF, XChaCha20-Poly1305, X25519, Ed25519, SHA-256, TLS) except
SLIP-0039. It is a community specification (a four round Feistel cipher over PBKDF2-HMAC-SHA256 with Shamir's scheme
and a custom checksum), not published by an international standards body. BIS defines "standard cryptography" by
such bodies, and Apple asks the same question. With SLIP-0039 in the build, FigLock could not use the
self-classification route and would need a classification from BIS first (and Apple would ask for it). Its code was
also linked into the iPhone library although no iPhone feature used it.

## Decision

- Remove SLIP-0039 altogether. The Recovery Key is the single `FLR1-` code on every platform: an X25519 key
  (`crypto.md` 4.2) written down once, on a one page PDF with a QR code.
- The desktop app no longer offers cards: no Shamir option in the Recovery Key setup or the forgot flow, one page
  PDF only.
- `figlock_core::slip39`, `RecoveryKey::split` and `from_shares`, the cards variant of the printable Recovery Key,
  the SLIP-0039 word list, its tests and vectors (`slip39.json`, `slip39-official.json`), the `shamir-mnemonic`
  vector dependency and the `pbkdf2` and `hmac` crates are deleted.
- No vault format changes: `vault.json` stores `rk_pub` and `wrapped_vk_rk` exactly as before. A Recovery Key made
  as a code keeps working. A Recovery Key set up with cards on an earlier build cannot be used from cards any more;
  its holder must set up a new code in Settings while the vault is unlocked (the old key stops working then, as for
  any replacement). Cards only existed in the desktop builds before this change.

## Consequences

- One less piece of cryptography to own and defend; every primitive in the product is now a published standard,
  which lets the iPhone app self-classify (`docs/ios-export-compliance.md`).
- A person who wanted to give recovery to several people loses that option for now. The single code can still be
  copied to more than one safe place, but anyone holding it can open the vault (`pairing-recovery.md` 7.4).
- The `slip39` entry of the vector set and the third-party notice for the SLIP-0039 word list are gone.
- FIG-243 looks for a standards-based way to give a group of people recovery. Nothing is built until that has an ADR.

## Alternatives considered

- **Keep SLIP-0039 on the desktop and compile it out of the iPhone build with a cargo feature.** Rejected: it
  keeps a non-standard scheme in the product and in the export story (the desktop and CLI are exported too), and
  needs feature plumbing across every crate the iPhone library links.
- **Another threshold scheme.** No Shamir variant is specified by the IETF, NIST or IEEE today, so nothing would be
  more standard than SLIP-0039 without a standards-body text to follow. Tracked in FIG-243.
- **Get a BIS classification and keep SLIP-0039.** Possible, but slower, ongoing, and for a feature used by few
  people and not at all on the iPhone.
