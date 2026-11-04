# ADR-0001: iOS first, Android right after

- Status: Accepted
- Date: 2026-11-03
- Decision: D1 in `docs/decisions.md`

## Context

The owner uses an iPhone and a Mac, and v1 is built for the owner first. Mobile autofill is platform specific and the hardest part of the client work (PRD risk table).

## Decision

Phase 5 builds iOS first (SwiftUI app with a credential provider extension and Face ID), then Android in the same phase with the same core, targeting parity by the end of the phase.

## Consequences

- The Mac plus iPhone pair is the first supported setup, matching the default storage advice in ADR-0002.
- Android is not dropped, only sequenced; the UniFFI bindings (ADR-0004) are built for both from the start so nothing in the core assumes Apple.
- Android users wait until phase 5 for a native app; the CLI, desktop and web (ADR-0005) cover them earlier.

## Alternatives considered

- Both platforms in parallel: doubles the mobile work before the core is proven.
- Android first: not the owner's device, so slower feedback.
