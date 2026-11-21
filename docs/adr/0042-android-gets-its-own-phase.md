# ADR-0042: Android gets its own phase after iOS

- Status: Proposed
- Date: 2026-11-22
- Decision: D42 in `docs/decisions.md`
- Supersedes in part: ADR-0001 (the "Android in the same phase" part only)
- Task: FIG-153

## Context

ADR-0001 put iOS first and Android "right after, in the same phase" with parity by the end of phase 5. Planning
the iOS work (FIG-153 to FIG-178) showed that phase 5 is already large: a Rust sync extraction, the app, an
AutoFill extension, the relay and push, the release pipeline, and a dogfood week. Android adds a second set of
the same, on a different toolchain and different autofill APIs, and most of what it should copy is not known
until iOS has shipped. Keeping both in one phase also means the phase exit test could not be met until both
platforms are done.

## Decision

- **Phase 5 is the iOS app only.** Its exit test: iOS autofill works, an agent grant is approved from the iPhone, and
  a TestFlight build runs on the owner's iPhone.
- **Phase 5b is the Android app**, sequenced after the iOS dogfood (FIG-176). Its exit test: Android reaches parity
  with iOS (autofill, agent approval from the phone, sync, a Play internal track build installed). Phase 6
  (public release) keeps its number, so no existing reference changes.
- **What stays shared.** The UniFFI bindings (ADR-0004) are still built for both from the start, and nothing in the
  core, the sync crate extraction or the relay may assume Apple. The relay and the approval protocol are
  platform neutral; APNs ships first and an FCM sender is added in phase 5b.
- **Android architecture is a decision for phase 5b** (FIG-179), taken with the iOS lessons in hand: storage,
  hardware keystore unlock, AutofillService against Credential Manager.
- **Store launch is one task in phase 6.** Each platform has its own signing and test-track release task in its
  phase; the public App Store and Play launch (FIG-55) waits for both dogfood tasks.

## Consequences

- iOS can ship and be dogfooded without waiting for Android.
- Android users wait longer for a native app. The CLI, desktop app and browser extension cover them in the
  meantime (ADR-0001 already accepted this).
- ADR-0001 is not rewritten. Its status line points here and D1 in `docs/decisions.md` says so.

## Alternatives considered

- **Keep both in phase 5:** one big exit test and no early iOS release.
- **Renumber phases so Android is phase 6:** correct order, but every doc, ticket and ADR that says phase 6 changes.
- **Drop Android from v1:** Android is a stated product requirement (PRD 7.6).
