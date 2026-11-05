# ADR-0020: App surfaces and UI stack

- Status: Accepted
- Date: 2026-11-05
- Decision: D20 in `docs/decisions.md`
- Supersedes: ADR-0005 (hosted PWA)

## Context

ADR-0004 fixed the core (one Rust core, UniFFI on mobile, native dependency on desktop, WASM for web) and ADR-0005 added a hosted PWA. Before building the apps we reviewed every surface against the same criteria: security, accessibility, fit with the Rust core, maintenance load and what the market does. The review comparison was kept on FIG-64.

## Decision

- **No hosted web app.** ADR-0005 is superseded. Phones use the native apps, and the only cases a PWA served (a borrowed or locked-down computer, a try-it link) are both weak fits: typing a master password on an untrusted machine is the riskiest use, and a static demo page on figlock.com plus the CLI install script cover the try-it case. Revisit only if users ask for it. The WASM core build stays because the extension needs it.
- **Desktop:** Tauri 2 with a Svelte 5 UI, core as a native Rust dependency. Linux accessibility (WebKitGTK) is checked with Orca before the first Linux release; if it fails, Linux ships as CLI plus extension until fixed.
- **Browser extension:** WXT with the WASM core, UI from `packages/ui`.
- **Shared UI:** `packages/ui` (Svelte 5 + Tailwind on `design/tokens.css`) serves desktop and extension only. iOS (SwiftUI) and Android (Compose) take the tokens, not components.
- **Passkey-PRF unlock (FIG-49) is deferred.** It only replaces typing the master password, adds a second key-wrapping path and a recovery story, and browser support from extension pages is unverified. The extension unlocks with the master password and keeps the session key in `chrome.storage.session` with an auto-lock timeout. A later biometric route is native messaging to the desktop app (Touch ID), which reuses FIG-43 and needs no new crypto path. Revisit after the extension and desktop ship, if users ask.
- **Safari on iOS** ships inside the iOS app (FIG-52), so it is sequenced after the iOS app.
- **Accessibility baseline:** WCAG 2.2 AA on web-technology surfaces with axe checks in Playwright plus a manual screen reader pass per release (VoiceOver, NVDA, Orca); platform guidelines on iOS and Android.

## Why not Flutter

Flutter would replace SwiftUI, Compose and Tauri with one Dart codebase for five OS targets, and that saving is real. We are not using it because:

- **Autofill stays native.** The iOS AutoFill credential provider and the Android autofill service are separate native components with strict memory and lifecycle limits, so Swift and Kotlin are needed anyway for the most important mobile feature. Most of the "one codebase" saving disappears.
- **Accessibility is emulated, not native.** Flutter draws its own controls and bridges them to the platform accessibility tree. Desktop and Linux are the weakest, and Dynamic Type, font scaling and focus behavior are reimplemented. A security tool used daily should not carry that risk.
- **A third core binding.** `flutter_rust_bridge` or Dart FFI would sit beside UniFFI (still required for the native autofill extensions) and WASM, plus a Dart runtime and plugin supply chain in the audit scope.
- **Secure input is reimplemented.** Custom-drawn text fields mean keyboard caching hints, password field semantics, screen-capture flags and clipboard handling go through plugins instead of platform controls.
- **It does not cover the extension.** Flutter web fits a Manifest V3 extension poorly (payload size, canvas rendering with poor accessibility, CSP friction), so a Svelte UI stays regardless.
- **Market.** As far as we know the password managers reviewed (Bitwarden, 1Password, Proton Pass, KeePassXC, Strongbox, Enpass) use native UI, Qt, Electron or a web stack for their main clients; none is known to us to be Flutter-based (not re-verified).

Flutter for the desktop shell only was also rejected: it gives up the native Rust dependency and the UI shared with the extension, adding a second UI stack for little gain. Revisit this ADR if the native UI cost proves unsustainable, with a spike measured against VoiceOver, TalkBack and Orca.

## Consequences

- One fewer deploy target and no hosted-JavaScript trust problem (threat model W1 no longer applies to a FigLock-served client; W2 still applies to the extension).
- Two UI stacks to maintain on top of the core: Svelte (desktop, extension) and native (iOS, Android), joined only by tokens.
- Users lose zero-install access from a borrowed machine; the CLI and extension remain.
- Task changes: FIG-47 shrinks to the WASM core build; FIG-49 (passkey PRF unlock) is deferred and set to low priority; FIG-43 gains the Linux accessibility check; FIG-52 carries the Safari web extension target; `apps/web/` is not created.
- `docs/spec/crypto.md` and `docs/spec/threat-model.md` mention the PWA as optional (MAY) or as a mitigation context; those texts stay valid if a web app is added later and are not changed here.

## Alternatives considered

- Keep the PWA (ADR-0005): weakest client, extra hosting and a third web UI target, no user it uniquely serves.
- Electron for desktop: consistent Chromium accessibility, but a larger attack surface, large binaries and a bridge to the Rust core.
- Native UI per desktop OS: best accessibility, three UIs to build and maintain.
- Plain Manifest V3 extension without WXT: more per-browser build glue by hand.
- Thin extension over native messaging: needs a host binary and installer on every OS; kept only as an optional biometric unlock helper later.
- Flutter, everywhere or desktop only: see above.
