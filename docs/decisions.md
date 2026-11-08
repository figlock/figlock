# Decisions

What is already agreed. Each line that changes direction later gets an ADR in `docs/adr/`.
Source: review rounds on the product plan and development plan, 2026-11-03 and 2026-11-04.

| #   | Date       | Question                     | Decision                                                                 | ADR |
| --- | ---------- | ---------------------------- | ------------------------------------------------------------------------ | --- |
| D1  | 2026-11-03 | First mobile platform        | iOS first (the owner uses an iPhone); Android right after, same phase 5 | [0001](adr/0001-ios-first.md) |
| D2  | 2026-11-03 | Default storage              | The user picks at setup. Suggest iCloud Drive for iPhone + Mac only, Google Drive for anything mixed | [0002](adr/0002-user-picks-storage.md) |
| D3  | 2026-11-03 | Agents seeing raw values     | Reveal is opt-in per item and needs its own approval every time          | [0003](adr/0003-agents-use-not-hold-secrets.md) |
| D4  | 2026-11-03 | Mobile approach              | Native: SwiftUI on iOS, Jetpack Compose on Android, shared Rust core via UniFFI | [0004](adr/0004-native-mobile-shared-rust-core.md) |
| D5  | 2026-11-03 | Hosted web app               | Superseded by D20 (was: an installable PWA at app.figlock.com)           | [0005](adr/0005-pwa.md) |
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
| D16 | 2026-11-04 | Where the repo lives first   | Started at `seemantshekhar43/figlock` (personal account, public); transferred to `figlock/figlock` on 2026-11-07 | [0016](adr/0016-personal-account-first.md) |
| D17 | 2026-11-04 | UI design tool               | OpenDesign (nexu-io/open-design, Apache-2.0), run locally. `design/DESIGN.md` + tokens in the repo are the brand contract for every app | [0017](adr/0017-opendesign.md) |
| D18 | 2026-11-05 | Pairing and revocation wire details | Request carries `device_id`; `revoked` and `epoch_keys` members in `vault.json`; rotation re-signs device records | [0018](adr/0018-pairing-and-revocation-wire-details.md) |
| D19 | 2026-11-05 | Broker protocol, grants and audit log | Unix socket with peer uid check, JSON lines, grants (who, what, mode, ttl) approved behind a trait, `reveal` always once; audit log in the file container (kind 0x03) under `AK` derived from `VK`, signed and hash-chained (proposed) | [0019](adr/0019-broker-protocol-grants-and-audit-log.md) |
| D20 | 2026-11-05 | App surfaces and UI stack    | No hosted web app (supersedes D5); desktop Tauri 2 + Svelte 5; extension WXT + WASM; shared `packages/ui` for desktop and extension only; native UI on iOS and Android; no Flutter; passkey-PRF unlock deferred | [0020](adr/0020-app-surfaces-and-ui-stack.md) |
| D21 | 2026-11-06 | Google OAuth client and sign-in | Desktop client; loopback + PKCE; `drive.appdata` only; client secret injected at build time (never in git), run-time env override; refresh token owner-only, revoked on logout | [0021](adr/0021-google-oauth-client-and-sign-in.md) |
| D22 | 2026-11-06 | Credential change after revocation | `Vault::change_credentials` rewraps the same `VK` under a new `AUK` (password, Secret Key, Argon2 parameters); `VK` is not rotated; run it right after a revocation | [0022](adr/0022-credential-change-after-revocation.md) |
| D23 | 2026-11-08 | Brand color, UI font and logo | Brand blue (`#3b5afe` light, `#9fb0ff` dark) replaces fig aubergine; Figtree (OFL) replaces Inter; radii one step larger; logo is an SVG set in `design/logo/`, source of truth | [0023](adr/0023-brand-blue-figtree-and-logo.md) |
| D24 | 2026-11-09 | Document items and per-field size limits | New `doc` item type holding files as base64 text (`file/<id>/name`, `mime`, `data`, up to 768 KiB each, 8 per item); every field has its own size cap, checked on write and on read; item limits (256 fields, 8 files) on write; no format change (proposed) | [0024](adr/0024-document-items.md) |
