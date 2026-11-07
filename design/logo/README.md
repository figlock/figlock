# FigLock logo and app icons

Source of truth for the FigLock mark, wordmark, lockups and app icons (FIG-46). Designed in OpenDesign, kept
here so every app, the site, store listings and the Google consent screen use the same files. Rules for use
are in `design/DESIGN.md` section 7.1. Open `index.html` for a contact sheet.

| Folder or file | Contents |
| --- | --- |
| `figlock-icon.svg`, `figlock-icon-flat.svg` | App icon (gradient and flat) |
| `mark/` | Padlock mark in color, black and white |
| `lockups/` | Wordmark, horizontal and stacked lockups, each for light and on-dark backgrounds |
| `app-icons/` | Favicon, iOS 1024, macOS 1024, maskable 1024, Android foreground, background and monochrome |
| `social/` | Round avatar and social card |

Do not edit these files by hand or recolor them. To change the logo, change it in OpenDesign, replace the
files here in one PR and update the ADR if the decision changes. PNG sizes for stores and platforms are
exported from the SVGs at release time.
