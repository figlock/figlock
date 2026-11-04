# ADR-0004: Native mobile apps with a shared Rust core via UniFFI

- Status: Accepted
- Date: 2026-11-03
- Decision: D4 in `docs/decisions.md`

## Context

Autofill, credential providers, biometrics and the keystore are platform APIs with strict lifecycle rules, and the security-critical code (crypto, merge, search) must not be written twice.

## Decision

iOS is SwiftUI and Android is Jetpack Compose, both calling the same Rust core (`figlock-core` through `figlock-ffi`) through UniFFI bindings. The web app uses the same core compiled to WASM. Desktop uses Tauri 2 with the core as a native dependency.

## Consequences

- One implementation of crypto, merge and search, tested against shared vectors on every platform.
- Each platform keeps native UI and system integration; UI code is not shared except tokens (`design/DESIGN.md` 10).
- Build complexity: Rust, Xcode and Gradle toolchains live side by side, managed by mise and just (FIG-8).

## Alternatives considered

- Cross-platform UI (Flutter, React Native): weaker autofill and credential provider support, an extra runtime to audit.
- Reimplementing the core per platform: multiplies the audit scope and the bug surface.
