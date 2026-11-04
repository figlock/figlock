# Security policy

FigLock is a password manager, so we take vulnerability reports seriously.

## Reporting a vulnerability

Please do not open a public issue for a security problem.

- Use GitHub's private vulnerability reporting: the **Report a vulnerability** button on the
  repository's Security tab, or
- email [security@figlock.com](mailto:security@figlock.com).

Include what you found, how to reproduce it, and the impact you expect. Never include real vault data,
tokens or recovery codes; use generated test values.

We aim to acknowledge a report within 3 working days and to share a plan or fix within 30 days. We
credit reporters in the advisory unless you prefer to stay anonymous.

## Scope

In scope: the crypto and sync specs in `docs/spec/`, the Rust crates, the CLI and broker, and the
desktop, web, extension and mobile apps, once they exist.

Out of scope: vulnerabilities in the storage providers themselves, and issues that need an already
compromised device or an unlocked vault.

## Status

FigLock is in planning and has had no security audit yet. Do not store real secrets in anything built
from this repository until the audit is done.
