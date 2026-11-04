# FigLock design system

Status: v1 (FIG-22). This file and `tokens.css` are the brand contract: OpenDesign reads them, and every
app (web, desktop, extension in `packages/ui`; iOS and Android natively) builds from them. If a screen
needs something this file does not allow, change this file first. Logo and final wordmark are a separate
decision (FIG-46); everything below works with a placeholder mark.

## 1. Principles

1. **Calm and trustworthy.** A password manager earns trust by being quiet. No gradients behind text, no
   playful animation, no alarm colors unless something is actually wrong.
2. **Secrets are visible only on purpose.** Hidden by default, shown for a short time after a deliberate
   action, never on screenshots or in the task switcher.
3. **Say exactly what will happen.** Prompts and buttons name the object and the effect ("Approve Claude
   Code to use *GitHub token* for 15 minutes"), not "Allow" and "OK".
4. **Fast first.** Search and unlock are the product. Keyboard first on desktop, thumb reach on phones.
5. **One system, many shells.** Same tokens and the same words on every platform, native feel where the
   platform has strong conventions (navigation, sheets, biometrics).

## 2. Voice

- Plain, short, direct. Second person ("your vault"), present tense, no jargon in user-facing text.
- Honest about limits: "This stops accidental leaks. An agent with shell access could still print it."
- Calm in errors: say what happened and what to do next. Never blame the user, never use exclamation marks,
  never use fear language ("hackers", "danger"). Use "can't" and "won't", not "unable to".
- Sentence case everywhere (buttons, titles, menus). No em dash character; use a plain dash `-`.
- Use the product's own terms consistently: **vault**, **item**, **Secret Key**, **Emergency Kit**,
  **device**, **agent**, **grant** (an approval for an agent), **broker**. Do not say "account", "login" for
  the whole product, or "password" when the item is a token.
- Sample content in design files is always made up: `Cloudflare`, `ghp_EXAMPLEexampleEXAMPLE...`. Never real
  data (AGENTS.md).

## 3. Color

Use only semantic tokens (`--color-*`) in components. The raw palette (fig, warm neutral) exists to define
them. The brand color is **fig**, a muted aubergine; everything else is warm neutral. **Agent teal** is
reserved for anything about an AI agent (grant sheets, agent audit entries) so users learn "teal means an
agent is involved".

### 3.1 Semantic tokens and contrast

Ratios are WCAG 2.x contrast computed from the hex values (checked by `design/check-contrast.py`, run by `just check` and CI). Targets: normal text
at least 4.5:1, large text and UI boundaries at least 3:1.

| Token | Light | Ratio on bg | Dark | Ratio on bg |
| ----- | ----- | ----------- | ---- | ----------- |
| `--color-text` | #1b191d | 16.6 | #f2f0f1 | 16.6 |
| `--color-text-secondary` | #5a555c | 6.9 | #c4bfc3 | 10.4 |
| `--color-text-tertiary` | #6b656d | 5.4 | #a29ca0 | 7.0 |
| `--color-primary` / link | #663c74 | 8.1 | #d3aedd | 9.7 |
| `--color-text-on-primary` on primary | #ffffff | 8.5 | #121114 | 9.7 |
| `--color-agent` | #0f6e7a | 5.7 | #5cc8d6 | 9.6 |
| `--color-success` | #1e7a4d | 5.1 | #5cd08f | 9.7 |
| `--color-warning` | #8a5a00 | 5.6 | #e8b04a | 9.6 |
| `--color-danger` | #b3261e | 6.2 | #ff8a80 | 8.2 |
| `--color-border-strong` (controls) | #8d878f | 3.3 | #6f6972 | 3.5 |

Background (`--color-bg`) is #faf9f9 in light and #121114 in dark. Surfaces step up from it
(`surface`, `surface-raised`); `surface-sunken` is for wells and code blocks.

### 3.2 Rules

- Color is never the only signal. Pair status colors with an icon and text (a red field also shows an error
  icon and message; a weak password also says "Weak").
- Primary is for the single main action on a screen. Danger styling is for destructive actions only
  (delete vault, revoke device) and always asks for confirmation.
- Light and dark are both first-class and tested. Follow the OS by default; the user can override.
  Components must not hard-code a theme.
- Status badges use the `*-subtle` background with the matching text color, which also meets 4.5:1.

## 4. Typography

- **UI:** Inter (variable, self-hosted, OFL) on web, desktop and extension; SF Pro on iOS and Roboto on
  Android. The system stack in `--font-sans` is the fallback.
- **Secrets, tokens, codes and the Secret Key:** a monospace font (`--font-mono`, JetBrains Mono), always
  with tabular, unambiguous glyphs. Group long values (`FL1-ABCD-EFGH-...`) so they can be read aloud and
  typed. Never use ligatures.

| Role | Size token | Weight | Line height | Use |
| ---- | ---------- | ------ | ----------- | --- |
| Display | `--text-4xl` 36 | 700 | tight | marketing and onboarding only |
| Title 1 | `--text-3xl` 30 | 700 | tight | screen titles (web, onboarding) |
| Title 2 | `--text-2xl` 24 | 600 | tight | section titles |
| Title 3 | `--text-xl` 20 | 600 | snug | dialogs, item title in detail view |
| Headline | `--text-lg` 18 | 600 | snug | list group headers |
| Body | `--text-md` 16 | 400 | normal | mobile and web body |
| Body (dense) | `--text-base` 14 | 400 | normal | desktop lists and forms |
| Label | `--text-sm` 13 | 500 | snug | field labels, buttons |
| Caption | `--text-xs` 12 | 500 | snug | metadata, timestamps, badges |

- Minimum text size is 12 px (12 pt on iOS, 12 sp on Android), and body text scales with the user's
  system text size setting. Layouts must survive 200 percent text.
- Line length for prose is 45 to 75 characters.

## 5. Spacing, layout and shape

- 4 px base grid. Use the `--space-*` scale only; there are no one-off paddings.
- Common patterns: 16 px page padding on phones, 24 px on desktop; 8 px between a label and its field;
  12 px between related controls; 24 to 32 px between sections.
- **Radius:** `xs` 4 (badges, chips), `sm` 6 (inputs, buttons), `md` 10 (cards, list rows), `lg` 14 (sheets,
  dialogs), `xl` 20 (large phone sheets), `full` (avatars, switches). Nested elements use a smaller radius
  than their container.
- **Elevation:** prefer borders and surface steps to shadows. Shadows (`--shadow-sm|md|lg`) only for
  floating layers: menus, popovers, sheets, toasts.
- **Controls:** minimum height 36 px on desktop, 44 px on iOS, 48 dp on Android. Hit areas are at least 44
  by 44 pt (iOS) and 48 by 48 dp (Android) even when the visible control is smaller.
- **Breakpoints (web):** compact < 640 px (phone layout), medium 640 to 1024 (single pane with a slide-over
  detail), expanded > 1024 (list plus detail, optional sidebar).

## 6. Motion

- Purpose only: show where something came from or went, or confirm a state change. No decorative loops.
- Durations: `instant` 80 ms (hover, press), `fast` 120 ms (small state), `base` 200 ms (menus, sheets
  entering), `slow` 320 ms (full screen transitions). Anything longer needs a reason.
- Easing: `--ease-standard` for moves, `--ease-enter` for appearing, `--ease-exit` for leaving.
- `prefers-reduced-motion` (and the iOS and Android equivalents): remove movement and fades longer than
  instant; keep the state change itself. Tokens already set durations to 0.
- The 30 second clipboard clear and grant countdowns are shown as text and a thin progress bar, never as an
  animation that is the only indicator.

## 7. Iconography

- One set: **Lucide** (ISC licence) on web, desktop and extension; SF Symbols on iOS and Material Symbols on
  Android, mapped one to one by name in `design/icons.md` (added with the first screens).
- Stroke style, 1.5 px at 24 px, rounded joins; 16, 20 and 24 px sizes. Icons inherit text color; status
  icons use the semantic color of their status.
- Every icon-only button has a visible tooltip or label on desktop and an accessible name on all platforms.
- Reserved meanings: padlock = vault locked state only; key = Secret Key and key items; shield-check =
  approved or healthy; bot = anything about an agent (always in agent teal); fingerprint = biometric.

## 8. Components with special rules

- **Secret field.** Masked by default (fixed-length dots, so length is not leaked). "Show" reveals for 10
  seconds and then re-masks; copy clears the clipboard after 30 seconds and says so. Revealed values use the
  mono font. On iOS and Android secret screens are blanked in the task switcher and screenshots are blocked
  where the platform allows.
- **Agent approval sheet.** The most security-sensitive screen. It is always drawn by FigLock (never by the
  agent), shows in this order: who is asking (agent name and how it was identified), what (item or scope),
  how (use it through HTTP, inject it, or reveal it), where (allowed hosts), for how long, and the actions
  **Approve with Touch ID / Face ID** and **Deny**. "Reveal" is styled in warning with its own sentence,
  never the default option, and always needs its own approval. Agent teal marks the sheet. No timers that
  pressure the user, no dark patterns.
- **Search.** Always one keystroke away (`/` or Cmd+K on desktop, pull down on mobile). Results update as the
  user types, show why an item matched (alias, domain, typo-corrected), and never display secret values.
- **Emergency Kit and Secret Key.** Printed layout uses the mono font with grouping and a clear
  "Keep this safe, it is shown only once" notice. Creation requires a "verify you saved it" step
  (re-enter the last group).
- **Destructive actions** (delete item, revoke device, reset vault) name the object, describe the effect in
  one sentence, and use danger styling on the confirm button only.
- **Empty and error states** explain what the user can do next in one sentence and offer the action.

## 9. Accessibility

- Meets WCAG 2.2 AA on web and the platform accessibility guidelines on iOS and Android.
- Text contrast per section 3; UI boundary and focus contrast at least 3:1 against adjacent colors.
- Visible focus on every interactive element: `--focus-ring-width` ring in `--color-focus` with
  `--focus-ring-offset`. Never remove outlines without replacing them.
- Everything works with keyboard only on desktop and web: logical tab order, no traps, Escape closes
  overlays, shortcuts are listed in a help sheet and never conflict with screen readers.
- Screen readers: every control has an accessible name, state and role. Secret values are announced only on
  an explicit Show action; masked fields announce "hidden" and their length is not announced. Live regions
  announce copy confirmations and errors.
- Target sizes per section 5. Supports system text scaling, bold text, increased contrast, reduced motion and
  reduced transparency.
- Forms: visible labels (not placeholder-only), errors linked to fields, no time limits except the
  documented security timeouts, which warn before locking and can be extended.
- Test with VoiceOver, TalkBack and one desktop screen reader per release; add automated axe checks to
  Playwright UI tests.

## 10. Platform mapping

Tokens are authored in CSS and generated by name to other platforms. The mapping is mechanical:

| Token group | Web, desktop, extension | SwiftUI | Jetpack Compose |
| ----------- | ----------------------- | ------- | --------------- |
| Colors | `var(--color-*)` | `Color("figlock/<name>")` in an asset catalog with light and dark variants, wrapped by `enum FLColor` | `FigLockColors` data class provided through `CompositionLocal`, with `lightColors()` and `darkColors()` |
| Type | `--text-*`, `--weight-*`, `--leading-*` | `Font.custom("Inter", size:relativeTo:)` or system font with `.dynamicTypeSize` | `TextStyle` set in `FigLockTypography`, sizes in `sp` |
| Spacing | `--space-*` (rem) | `enum FLSpace { static let s4: CGFloat = 16 }` | `object FLSpace { val s4 = 16.dp }` |
| Radius | `--radius-*` | `RoundedRectangle(cornerRadius:)` with `FLRadius` constants | `RoundedCornerShape(FLRadius.md)` via `Shapes` |
| Motion | `--duration-*`, `--ease-*` | `Animation.timingCurve(0.2, 0, 0, 1, duration: 0.2)`; respect `accessibilityReduceMotion` | `tween(durationMillis = 200, easing = CubicBezierEasing(0.2f, 0f, 0f, 1f))`; respect `Settings.Global.ANIMATOR_DURATION_SCALE` |
| Icons | Lucide | SF Symbols | Material Symbols |

Rules for native shells:

- The token names are the contract. A native constant is named after the token (`color-primary` becomes
  `FLColor.primary`).
- Use native navigation, sheets, pickers and biometric prompts; do not recreate web patterns where the
  platform has its own.
- `rem` values are converted at 1 rem = 16 pt or 16 dp, and text additionally scales with the user setting.
- A generator that emits Swift and Kotlin constants from `tokens.css` is a later task; until then the
  tables above are updated by hand in the same PR as any token change.

## 11. Using this with OpenDesign

- Point OpenDesign at `design/DESIGN.md` and `design/tokens.css` for every prototype.
- Export prototypes and screenshots into `design/prototypes/` so tasks and PRs can link them.
- Prototypes are references, not shipped code. Implementations live in `packages/ui` and the native apps.
- Never import real vault data or personal details into OpenDesign. Use made-up sample items only.

## 12. Changing this file

Token or rule changes land by PR with the contrast numbers re-checked if any color changed. A change that
breaks an accessibility rule needs a written reason in the PR and the owner's approval.
