# ADR-0023: Brand blue, Figtree and the FigLock logo

- Status: Accepted
- Date: 2026-11-08
- Decision: D23 in `docs/decisions.md`

## Context

`design/DESIGN.md` v1 (FIG-22) set fig aubergine as the brand color and Inter as the UI font, with a placeholder
mark. While prototyping the key screens in OpenDesign (FIG-23) the owner chose a different look and asked for the
logo (FIG-46) in the same session. The contract has to match what the prototypes and the logo show (ADR-0017:
change `DESIGN.md` first when a screen needs something it does not allow).

The owner first asked for Airbnb Cereal as the UI font. Cereal is proprietary: it cannot be self-hosted or
shipped in the apps, and it was not installed on the design machine, so the prototype had been rendering the
Figtree fallback all along.

## Decision

- **Brand color:** blue. Light `#3b5afe` (hover `#2c46d6`, subtle `#eef1ff`), dark `#9fb0ff` (hover `#b8c4ff`,
  subtle `#161d4a`). It replaces fig aubergine for primary, link and focus. Everything else, including warm
  neutrals, agent teal and the status colors, is unchanged. Contrast still meets WCAG 2.2 AA and is checked by
  `design/check-contrast.py`: primary on bg 4.9 (light) and 9.1 (dark), on-primary 5.2 and 9.1, primary on its
  subtle background 4.57 and 7.7.
- **UI font:** Figtree (SIL OFL, self-hostable), closest open font to the Cereal look the owner wanted. It
  replaces Inter. JetBrains Mono stays for secrets, tokens and codes. SF Pro and Roboto stay the native fonts.
- **Radii:** one step larger, xs 6, sm 10, md 14, lg 20, xl 28 (was 4, 6, 10, 14, 20), as drawn in the prototype.
- **Logo:** a white padlock with a small "f" on a blue rounded square, with a Figtree Bold wordmark, stored as
  SVG in `design/logo/` (mark, lockups, app icons, favicon, social files) and treated as the source of truth.
  The site favicon uses it.
- **Prototype:** `design/prototypes/key-screens/index.html` is the reference for the seven key screens.

## Consequences

- `design/DESIGN.md`, `design/tokens.css` and its byte-identical copy `site/tokens.css` change in the same PR.
  The site picks up the new color and radii on its next deploy.
- Native token tables in `DESIGN.md` section 10 keep their mapping; only the font name in the SwiftUI row
  changed. The Swift and Kotlin constants are still hand-maintained.
- Figtree must be self-hosted when real UI is built (`packages/ui`, desktop, extension). The prototype loads it
  from Google Fonts, which is fine for a reference but not for the shipped apps or the site CSP.
- Store listings, the Google consent screen (FIG-68) and platform icon PNGs are exported from `design/logo/`.
- The "f" mark and the blue are the owner's call and can be changed later with a new ADR; the files in
  `design/logo/` are replaced in one PR when that happens.

## Alternatives considered

- Keep fig aubergine and Inter: matches the v1 contract but not what the owner picked while designing.
- Airbnb Cereal: the owner's first choice, but proprietary, so it can't ship.
- Inter: neutral and already specified, but further from the warmer look the owner wanted.
