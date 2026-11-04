# Decisions

What is already agreed. Each line that changes direction later gets an ADR in `docs/adr/`.
Source: review rounds on the product plan and development plan, 2026-11-03 and 2026-11-04.

| #   | Date       | Question                     | Decision                                                                 | ADR |
| --- | ---------- | ---------------------------- | ------------------------------------------------------------------------ | --- |
| D1  | 2026-11-03 | First mobile platform        | iOS first (the owner uses an iPhone); Android right after, same phase 5 | [0001](adr/0001-ios-first.md) |
| D2  | 2026-11-03 | Default storage              | The user picks at setup. Suggest iCloud Drive for iPhone + Mac only, Google Drive for anything mixed | [0002](adr/0002-user-picks-storage.md) |
| D3  | 2026-11-03 | Agents seeing raw values     | Reveal is opt-in per item and needs its own approval every time          | [0003](adr/0003-agents-use-not-hold-secrets.md) |
| D4  | 2026-11-03 | Mobile approach              | Native: SwiftUI on iOS, Jetpack Compose on Android, shared Rust core via UniFFI | [0004](adr/0004-native-mobile-shared-rust-core.md) |
| D5  | 2026-11-03 | Hosted web app               | Yes, an installable PWA at app.figlock.com                               | [0005](adr/0005-pwa.md) |
| D6  | 2026-11-03 | Relay for phone approvals    | Yes, optional and stateless (Cloudflare Worker); everything works without it | [0006](adr/0006-optional-stateless-relay.md) |
| D7  | 2026-11-03 | License                      | Free, dual MIT OR Apache-2.0                                             | [0007](adr/0007-license.md) |
| D8  | 2026-11-03 | Agent interface              | Claude skill + `figlock` CLI first; MCP adapter is optional, in phase 6  | [0008](adr/0008-skill-and-cli-first.md) |
| D9  | 2026-11-03 | Product name                 | FigLock. figlock.com registered on Cloudflare 2026-11-03                 | [0009](adr/0009-product-name.md) |
| D10 | 2026-11-03 | Google Drive scope           | Only `drive.appdata` (non-sensitive): hidden app folder, no access to the user's files | [0010](adr/0010-drive-appdata-scope.md) |
| D11 | 2026-11-04 | Project tracker              | Paperclip, run locally in loopback mode with telemetry off               | [0011](adr/0011-paperclip-tracker.md) |
| D12 | 2026-11-04 | Repo visibility              | Public from day 1                                                        | [0012](adr/0012-public-repo.md) |
| D13 | 2026-11-04 | Repo owner                   | Superseded by D16                                                        | [0013](adr/0013-repo-owner-superseded.md) |
| D14 | 2026-11-04 | GitHub Issues                | Every task gets a GitHub issue when work on it starts (see AGENTS.md)    | [0014](adr/0014-github-issue-per-task.md) |
| D15 | 2026-11-04 | Repo structure (proposed)    | One monorepo (`seemantshekhar43/figlock`, later `figlock/figlock`) for core, CLI, apps, relay and the figlock.com site; separate repos only for `homebrew-tap` and `.github` | [0015](adr/0015-monorepo.md) |
| D16 | 2026-11-04 | Where the repo lives first   | `seemantshekhar43/figlock` (personal account, public). Transfer to a `figlock` org once there is something concrete, before the first store or Homebrew release | [0016](adr/0016-personal-account-first.md) |
| D17 | 2026-11-04 | UI design tool               | OpenDesign (nexu-io/open-design, Apache-2.0), run locally. `design/DESIGN.md` + tokens in the repo are the brand contract for every app | [0017](adr/0017-opendesign.md) |
