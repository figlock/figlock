# Decisions

What is already agreed. Each line that changes direction later gets an ADR in `docs/adr/`.
Source: review rounds on the product plan and development plan, 2026-11-03 and 2026-11-04.

| #   | Date       | Question                     | Decision                                                                 |
| --- | ---------- | ---------------------------- | ------------------------------------------------------------------------ |
| D1  | 2026-11-03 | First mobile platform        | iOS first (the owner uses an iPhone); Android right after, same phase 5 |
| D2  | 2026-11-03 | Default storage              | The user picks at setup. Suggest iCloud Drive for iPhone + Mac only, Google Drive for anything mixed |
| D3  | 2026-11-03 | Agents seeing raw values     | Reveal is opt-in per item and needs its own approval every time          |
| D4  | 2026-11-03 | Mobile approach              | Native: SwiftUI on iOS, Jetpack Compose on Android, shared Rust core via UniFFI |
| D5  | 2026-11-03 | Hosted web app               | Yes, an installable PWA at app.figlock.com                               |
| D6  | 2026-11-03 | Relay for phone approvals    | Yes, optional and stateless (Cloudflare Worker); everything works without it |
| D7  | 2026-11-03 | License                      | Free, dual MIT OR Apache-2.0                                             |
| D8  | 2026-11-03 | Agent interface              | Claude skill + `figlock` CLI first; MCP adapter is optional, in phase 6  |
| D9  | 2026-11-03 | Product name                 | FigLock. figlock.com registered on Cloudflare 2026-11-03                 |
| D10 | 2026-11-03 | Google Drive scope           | Only `drive.appdata` (non-sensitive): hidden app folder, no access to the user's files |
| D11 | 2026-11-04 | Project tracker              | Paperclip, run locally in loopback mode with telemetry off               |
| D12 | 2026-11-04 | Repo visibility              | Public from day 1                                                        |
| D13 | 2026-11-04 | Repo owner                   | Superseded by D16                                                        |
| D14 | 2026-11-04 | GitHub Issues                | Every task gets a GitHub issue when work on it starts (see AGENTS.md)    |
| D15 | 2026-11-04 | Repo structure (proposed)    | One monorepo (`seemantshekhar43/figlock`, later `figlock/figlock`) for core, CLI, apps, relay and the figlock.com site; separate repos only for `homebrew-tap` and `.github` |
| D16 | 2026-11-04 | Where the repo lives first   | `seemantshekhar43/figlock` (personal account, public). Transfer to a `figlock` org once there is something concrete, before the first store or Homebrew release |
| D17 | 2026-11-04 | UI design tool               | OpenDesign (nexu-io/open-design, Apache-2.0), run locally. `design/DESIGN.md` + tokens in the repo are the brand contract for every app |
