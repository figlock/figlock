# ADR-0009: Product name FigLock; figlock.com on Cloudflare

- Status: Accepted
- Date: 2026-11-03
- Decision: D9 in `docs/decisions.md`

## Context

A short, available name was needed. About 2,000 candidates were checked against the .com registry and finalists on Cloudflare Registrar; `<word>vault.com` names are nearly all taken.

## Decision

The product is called FigLock and figlock.com was registered on Cloudflare on 2026-11-03. The names `figlock` on crates.io, npm, GitHub and Homebrew were free on 2026-11-04.

## Consequences

- Trademark and App Store and Play Store name checks are still open (FIG-13) and could force a rename before the first store release.
- Package names should be held early (FIG-12).
- DNSSEC and email routing are set on the Cloudflare zone (FIG-11).

## Alternatives considered

- Other candidate names: taken or too similar to existing vault products.
