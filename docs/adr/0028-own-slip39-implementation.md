# ADR-0028: SLIP-0039 is implemented in figlock-core, checked against the official vectors

- Status: Accepted
- Date: 2026-11-13
- Decision: D28 in `docs/decisions.md`

## Context

The optional Recovery Key (`pairing-recovery.md` 7) can be split into Shamir shares with SLIP-0039, so that
`M` of `N` cards open the vault when the master password is lost. The spec names the scheme and requires the
Rust code to pass the official vectors, but does not say where the implementation comes from.

The Rust crates for SLIP-0039 are not a safe base for a secret that opens a vault:

- `slip39-core` (Apache-2.0) was published two days before this decision and has a few hundred downloads.
- `sssmc39` (Apache-2.0) is marked work in progress and has not been updated since 2020.
- `slip39` is GPL-3.0-or-later, which is incompatible with our MIT OR Apache-2.0 license (ADR-0007).

SLIP-0039 itself is not new cryptography. It composes standard parts: PBKDF2-HMAC-SHA256, a Feistel network
over them, Shamir's scheme over GF(256) and a Reed-Solomon checksum, about 400 lines in the reference
implementation. The upstream project publishes 45 test vectors that cover valid and invalid share sets,
extendable backups, padding, digests and group thresholds.

## Decision

- `figlock_core::slip39` implements the SLIP-0039 share format, including groups, the extendable backup flag
  and the passphrase, from the specification and the reference implementation. PBKDF2 and HMAC come from the
  RustCrypto `pbkdf2` and `hmac` crates; nothing else cryptographic is written by hand.
- GF(256) arithmetic is table-free (shift and xor, fixed iteration counts), so no secret byte indexes memory.
- The official vectors from `trezor/python-shamir-mnemonic` (MIT, commit `9fcb9c8`) are committed unchanged as
  `docs/spec/vectors/slip39-official.json`, and a test requires every one of the 45 to pass: the valid ones to
  recover the stated master secret, the invalid ones to be rejected.
- The shares of `slip39.json` (made with the reference library) must also recover, and shares the Rust code
  makes must round trip through it for every `M` of `N`, with property tests over the secret, threshold and
  share count.
- FigLock only offers one group with threshold `M` of `N` (`2 <= M <= N <= 16`, default 2 of 3), an empty
  passphrase and the extendable flag on. Accepting the whole format keeps shares made by other tools
  recoverable and keeps the code checkable against every official vector.
- Combining is strict like the reference: the exact number of shares per group is required, identical shares
  count once, and any metadata mismatch, checksum failure, bad padding or bad digest is the one generic error.

## Consequences

- The project owns a small, tested piece of secret-sharing code instead of depending on an unproven crate. It
  must be kept as it is unless the SLIP-0039 specification changes, and any change needs the official vectors
  to keep passing.
- Two RustCrypto crates are added (`pbkdf2`, `hmac`; MIT OR Apache-2.0, already in the dependency tree through
  `hkdf` for `hmac`).
- The SLIP-0039 wordlist (public, part of the specification) is included as `src/slip39_words.txt`.
- A third party that reviews the crypto has a precise list: this module, its vectors, and the RustCrypto
  primitives it calls.
