# Research and findings

Findings behind the decisions in `docs/decisions.md`, gathered 2026-11-03 and 2026-11-04. Sources are at
the end of each section.

## 1. Problems we are solving

| Problem today                                        | What fixes it                                                                 |
| ---------------------------------------------------- | ----------------------------------------------------------------------------- |
| Can't remember passwords per site                    | Vault + generator + autofill (extension, iOS and Android autofill providers)   |
| Notes app isn't on the laptop                        | One encrypted vault synced through the user's own cloud storage                |
| GitHub / Vercel / Cloudflare tokens scattered in notes | First-class API token item (provider, env var, scopes, expiry, allowed hosts) + notes import with token detection |
| Can't find a secret when the name is forgotten        | Local fuzzy, typo-tolerant search over every non-secret field, plus token prefix (`ghp_`) and domain |
| Secrets pasted into agent chats                       | Local broker + Claude skill: agent asks, user approves with Touch ID, secret is injected, never shown |

## 2. Storage: bring your own, no backend

| Option                        | Web | CLI/desktop | Mobile   | Verdict                         |
| ----------------------------- | --- | ----------- | -------- | ------------------------------- |
| Google Drive appDataFolder    | Yes | Yes         | Yes      | Suggested for mixed devices      |
| Local / synced folder (iCloud Drive, Dropbox, Syncthing) | No | Yes | iOS only | MVP; suggested for iPhone + Mac |
| Private Git repo              | Via API | Yes     | Via API  | v1                              |
| S3 / R2 bucket                | Yes | Yes         | Yes      | Later                           |
| CloudKit                      | Partial | Apple only | iOS only | Skip: locks out Android/Windows |
| Pure peer-to-peer             | WebRTC | Yes      | Limited  | Pairing only                    |

Key finding: storage only needs `put / get / list / delete`. Each device writes only its own append-only
folder of encrypted, signed op files, so dumb storage never sees write conflicts. Clients merge with
per-field last-writer-wins registers ordered by a hybrid logical clock (a CRDT), with tombstones for
deletes. New devices pair through an `inbox/` folder in the storage itself.

Layout on storage:

```
figlock/
  vault.json                     vault id, KDF params, wrapped vault keys (signed)
  devices/<device-id>.json       device public keys, name, added-by signature
  log/<device-id>/00000042.op    encrypted, signed batch of changes; only this device writes here
  snap/<device-id>-00000040.snap compacted snapshot
  inbox/<request-id>.req         pairing requests and shares, encrypted to a recipient key
```

Sources: Google Drive appDataFolder docs; BlankDrive (encrypted vault on appDataFolder); Enpass
whitepaper (client-side merge sync); secsync, MindooDB, Automerge paper.

## 3. Crypto

- Two-secret key derivation: Argon2id(master password) combined with HKDF(128-bit Secret Key) gives the
  Account Unlock Key, which unwraps the Vault Key, which wraps per-item keys. A stolen cloud account plus
  a weak password still can't be brute-forced (same model as 1Password).
- Primitives: XChaCha20-Poly1305, X25519, Ed25519, `zeroize` on secret buffers.
- Quick unlock: Secure Enclave / Android Keystore / TPM releases a device-wrapped key after biometrics.
- Passkey PRF: an extra unlock method on the web only, never the sole root (PRF support and loss risks).
- Recovery: printable Emergency Kit with the Secret Key; optional Shamir split to trusted people. With no
  server, nobody can reset a forgotten password, so recovery is designed in from phase 0.

Sources: Bitwarden KDF algorithms; Corbado on passkey PRF and the risk of PRF as the only root.

## 4. Agent access

- Market: Bitwarden shipped an Agent Access SDK, and 1Password has "Security for AI" and secure AI access.
  Both confirm the need; neither covers a bring-your-own-storage, no-backend vault.
- Our model: a broker daemon (like `ssh-agent`) holds time-boxed grants approved with Touch ID.
  1. `figlock http`: broker adds the auth header itself, only for the token's allowed hosts. Safest.
  2. `figlock run`: env injection into a subprocess, output scrubbed. Stops accidental leaks.
  3. `figlock reveal`: opt-in per item, own approval every time. Escape hatch.
- Grant: who (agent identity from the process chain), what (item, tag or vault), how (mode), how long
  (once, 15 min, session, 1 day, until lock), recorded in an encrypted audit log.
- Skill + CLI instead of MCP: no extra server, loads only when needed, works with any shell agent. MCP
  stays an optional adapter for shell-less chat clients (phase 6).
- Honest limit: env injection can't stop a determined agent with shell access from printing a transformed
  value. Only broker HTTP with host allow-lists holds up against active exfiltration.

Sources: Bitwarden Agent Access SDK announcement and blog; 1Password Security for AI.

## 5. Search

Local only, after unlock: in-memory index over an allow-list of non-secret fields (titles, URLs, usernames,
tags, notes, provider, ...); fuzzy and typo-tolerant matching with an in-crate matcher (no nucleo
dependency); domain normalization; token prefix lookup by provider; caller supplied aliases (tags already
work as aliases, the schema has no alias field); recency boost. Nothing secret is indexed, the token value
included. Details are in the `figlock-core` `search` module docs.

## 6. Google Drive setup

Our side (once): one Google Cloud project (all platform clients must share it because the hidden folder
belongs to the project), Drive API on, consent screen with name, logo, support email, figlock.com
homepage, privacy and terms pages, figlock.com verified in Search Console, scope `drive.appdata` only.
OAuth clients: Desktop (CLI + Tauri, loopback + PKCE), Web (PWA, token model), Chrome extension, iOS,
Android. Testing status allows 100 test users and sign-ins expire after 7 days; production needs only
basic brand verification because `drive.appdata` is non-sensitive (no CASA security assessment).

User side: pick Google Drive, sign in once per device, pair the device; the data is hidden from the Drive
file list, counts against their storage (a few MB) and can be deleted from Drive > Settings > Manage apps.

Sources: Google "Choose Drive API scopes"; Google OAuth app audience (Testing vs In production).

## 7. Name and domain

- About 2,000 candidate names were checked against the .com registry (whois), and finalists were verified
  on Cloudflare Registrar. `<word>vault.com` is about 98% taken.
- Chosen: FigLock. figlock.com registered on Cloudflare 2026-11-03 ($11.17/yr).
- `figlock` is free on crates.io, npm, GitHub (user/org) and Homebrew as of 2026-11-04.
- Still to check: trademarks and App Store / Play Store name availability.

## 8. Project tracker: Multica vs Paperclip

| Need                    | Multica                                   | Paperclip                                          |
| ----------------------- | ----------------------------------------- | -------------------------------------------------- |
| Run locally             | Docker + Postgres                         | One `npx` command, embedded Postgres, no Docker    |
| License                 | Apache-2.0 plus extra conditions          | MIT                                                |
| Phases / milestones     | Projects + sub-issue stages               | Goals, projects, milestones, issues                |
| Blockers                | Only a Blocked status                     | First-class blocks / blocked-by, dependency wake   |
| Views                   | List, board, table, Gantt, swimlane       | Board and list                                     |
| Assets and docs         | Versioned deliverables                    | Documents with revisions, attachments, work products |
| PR linking              | Automatic, Done on merge                  | Manual via task ID (GitHub bot needs Paperclip Cloud) |
| GitHub Issues sync      | No                                        | No (roadmap)                                       |
| Agent cost control      | Usage analytics                           | Budgets with hard stops                            |

Chosen: Paperclip, because it runs locally with one command, has real blocker links and a hierarchy that
matches our phases. Neither syncs GitHub Issues, so we open a GitHub issue when a task starts.

Sources: multica-ai/multica README, issues and GitHub integration docs; paperclipai/paperclip README,
`doc/TASKS.md`, connectors docs, `doc/connections/GITHUB.md`.

## 9. Repo structure

Monorepo vs one repo per app:

| Factor                         | Monorepo                                         | Repo per app                                  |
| ------------------------------ | ------------------------------------------------ | --------------------------------------------- |
| Core change touching all apps  | One PR, one review, one CI run                    | Publish core, then bump it in 5+ repos        |
| FFI bindings (Swift, Kotlin, WASM) | Generated and tested against the same commit | Version skew between core and each app        |
| Tasks and GitHub issues        | One issue tracker, one label set                  | Issues scattered across repos                 |
| Agents                         | See specs, core and apps together                 | Need several checkouts for one change         |
| Security audit                 | One scope, one commit hash                        | Many repos to pin and audit                   |
| CI cost                        | Needs path filters so iOS CI doesn't run for a site typo | Naturally separate                     |
| Toolchains                     | Rust + Node + Xcode + Gradle side by side (mise / just keep it sane) | One toolchain per repo |

Recommendation: one monorepo (`seemantshekhar43/figlock` now, `figlock/figlock` after the transfer) that also holds `site/` (figlock.com, deployed by
Cloudflare Pages with build watch paths on `site/`) and `relay/`. There is no backend to separate.
Separate repos only where tooling demands it: `homebrew-tap` and `.github`.

Where it lives first: the owner's personal account (`seemantshekhar43/figlock`), transferred to a
`figlock` org later. A GitHub transfer keeps issues, PRs, wiki, stars, watchers and releases, and old
URLs redirect to the new location. To keep the move painless: publish install and docs links on
figlock.com instead of github.com paths, update the crates.io `repository` field and the Homebrew tap
after the move, and re-check Actions secrets, Pages and branch rules on the new owner. Doing it before
the first store or Homebrew release avoids most of that.

## 10. UI design tool: OpenDesign

- nexu-io/open-design: Apache-2.0, about 106k stars, active daily (last push 2026-11-04).
- Local-first desktop app for macOS and Windows (Linux from source). It doesn't ship a model; it drives
  the agent CLIs on the machine, and Claude Code connects with `od mcp install claude`.
- Every render reads a `DESIGN.md` brand contract; newer design-system packages add `tokens.css`,
  components and assets. 151 design systems ship with it.
- Outputs: HTML prototypes (web, desktop, mobile), slides, images, video. It can also refactor an
  existing codebase to a `DESIGN.md`.
- Fit for FigLock: design system, key screens (unlock, item list, item detail, agent approval sheet,
  pairing, Emergency Kit), store screenshots, figlock.com pages, logo exploration.
- Caveats: "scrubbed safety and reliability telemetry is always on", and cloud model calls leave the
  machine, so only sample data goes in. HTML prototypes translate directly to the web, desktop and
  extension UI; for SwiftUI and Compose they are visual references and the tokens carry over.
- On macOS the shell may resolve `od` to Apple's octal-dump tool; use the MCP snippet from the app's
  settings instead of the bare command.
