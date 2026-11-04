# ADR-0007: License: MIT OR Apache-2.0

- Status: Accepted
- Date: 2026-11-03
- Decision: D7 in `docs/decisions.md`

## Context

The project is free and public from day 1 (ADR-0012) and should be easy to adopt, embed and audit. The Rust ecosystem convention is dual MIT and Apache-2.0, which also gives an explicit patent grant.

## Decision

License all code under MIT OR Apache-2.0, at the user's option. `LICENSE-MIT` and `LICENSE-APACHE` are at the repo root, and contributions are accepted under the same terms (`CONTRIBUTING.md`).

## Consequences

- Anyone can use, fork and embed the code, including commercially.
- We cannot later prevent a hosted competitor without relicensing, which would need consent of contributors.
- Dependencies must be compatible; `cargo-deny` enforces an allow-list of licenses.

## Alternatives considered

- AGPL or similar: discourages adoption and embedding.
- Source-available: conflicts with the trust model of an auditable password manager.
