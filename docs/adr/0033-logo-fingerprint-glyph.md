# ADR-0033: The logo glyph is a fingerprint, not an f

- Status: Accepted
- Date: 2026-11-14
- Decision: D33 in `docs/decisions.md`
- Changes: the mark chosen in ADR-0023

## Context

ADR-0023 set the logo as a white padlock with a small "f" on a blue rounded square. Google's branding
verification for the OAuth consent screen (FIG-68) rejected it: the logo "does not uniquely identify your brand
and identity" and must not impersonate another brand. A white lowercase f on a blue rounded square reads as
another company's app icon.

## Decision

- **Only the glyph changes.** The padlock, the blue rounded square, the gradient, the wordmark and the lockup
  layouts stay as they were.
- **The glyph is three concentric fingerprint arches** in the brand blue `#3050E4`, stroke 22 on the 1024 icon
  grid. It points at what FigLock actually does: a secret is used after the person approves with Touch ID.
  The favicons use a thicker stroke so the arches survive at 16 px.
- **No letter and no stock security symbol.** We compared a script f, FL initials, password dots, asterisks,
  redacted text lines, a fig, a key, binary digits, a shield with a check and the fingerprint arches in
  OpenDesign. Letters and shields are either lookalikes or generic, so they would fail the same test.
- **Replaced in one PR.** Every SVG in `design/logo/` and its copies, the site favicon and lockups, the desktop
  app icons and the site social card are regenerated from the new SVGs, as the logo README requires.

## Consequences

- The consent screen gets a new 120x120 PNG exported from `design/logo/figlock-icon.svg` and branding
  verification is resubmitted.
- The arches are line work, so they are weaker than a solid shape at 16 px; the padlock carries the favicon
  there.
- If Google rejects this too, the next step is a more specific mark (for example a fig), not another letter.

## Alternatives considered

- **Script f, 64 percent size, 60:40 vertical placement:** distinct from the old f but still an f, and the
  closest to the rejected mark.
- **FL monogram:** spells the name but reads as a model number and is weak at 16 px.
- **Shield with a check:** the most legible at small sizes, but the most generic security glyph.
- **Password dots, asterisks, redacted lines:** say "secret" but are common in other password managers or read
  as UI icons.
- **A fig:** the most ownable, but needs a proper illustration; kept as a later option.
