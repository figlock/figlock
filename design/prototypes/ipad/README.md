# FigLock for iPad - prototype (FIG-264)

Reference prototype for the iPad app, built in OpenDesign from `design/DESIGN.md` and `design/tokens.css`, following
ADR-0048 (universal build, three column split view, sheets and popovers, several windows, one lock). It is a reference,
not shipped code (`design/DESIGN.md` 11): the SwiftUI screens are built from it in FIG-266 and the tasks after it.

Open `index.html` in a browser. It is one self-contained file with no network requests. All sample data is made up.

## Controls

- **Device size:** 11 inch (834 x 1194 pt) or 13 inch (1032 x 1376 pt), drawn at true point size and scaled to fit.
- **Orientation:** portrait or landscape.
- **Theme:** light, dark or system.
- **Multitasking:** full screen, Split View 1/2, Split View 1/3 (compact width), Slide Over.
- **Text size:** default, large, AX5 (the Dynamic Type check: columns collapse and rows wrap, nothing truncates).
- **Biometric:** Face ID or Touch ID wording and icon (ADR-0048 decision 7).
- **Reduce motion:** removes sheet and sidebar transitions and keeps the state change.
- **Screen:** welcome, pairing, unlock (Face ID and Touch ID), recovery, vault with an item selected, vault with nothing
  selected, item detail, edit sheet, generator popover, Settings page sheet, Devices, Sync, Recently deleted, agent
  approval card (and the Reveal variant), AutoFill popover, privacy cover, keyboard shortcut overlay, delete confirmation.

## Layout rules shown

- Regular width: sidebar, item list and detail. The sidebar is docked in 11 inch landscape and on the 13 inch, and
  starts hidden in 11 inch portrait. Compact windows (narrow Split View, Slide Over) use the iPhone layout.
- Edit is a form sheet, Settings a page sheet, the generator and delete confirmation are popovers (bottom sheets in
  compact width). Onboarding, pairing and unlock are a centred column, not sheets.
- Recently deleted is one wide list (no detail column); a row opens a restore or delete forever popover.

## Screenshots

`screens/` holds 29 captures of the states above (sizes, orientations, themes, Split View and Slide Over, AX5 and
Large text, Face ID and Touch ID, every sheet and popover). Each shows the controls bar, so the state is readable.

## How it was checked

All 960 combinations of size, orientation, multitasking, text size and screen were loaded in a browser and checked for
clipped or ellipsised text, content outside the screen, and tap targets under 44 pt. The only remaining findings are
screen-reader-only headings and, in the narrowest windows (278 pt, 11 inch portrait Split View 1/3), a few
ellipsised subtitles, which matches the iPhone list.

## Simulated, not real

The Face ID and Touch ID prompts, the Safari page behind the AutoFill popover and the create vault and Google Drive
buttons are mock-ups. The prototype source of truth is the OpenDesign project "FigLock iOS app screens".
