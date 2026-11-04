# FigLock - product requirements (PRD)

Status: draft v1, 2026-11-04. Owner: seemantshekhar43 on GitHub. Decisions referenced as D<n> are in
`docs/decisions.md`; findings are in `docs/research.md`.

## 1. Summary

FigLock is a free, open-source password and secrets manager for one person and their AI agents. It keeps
passwords, multiple logins per site, API tokens and notes in one encrypted vault that syncs through
storage the user already owns, with no FigLock server or database. Agents can use a secret after the user
approves with Touch ID, for a limited scope and time, without the secret entering the chat.

## 2. Problems

1. Passwords are hard to remember per site, so people reuse them.
2. Notes live on the phone, not the laptop.
3. API tokens (GitHub, Vercel, Cloudflare) are scattered across notes.
4. Things saved under a forgotten name can't be found.
5. Secrets get pasted into Claude and other agents, ending up in transcripts and provider logs.

## 3. Users

- **Primary:** a developer who uses an iPhone and a Mac (later Windows/Linux), works with Claude Code and
  other agents daily, and has tokens spread across notes. This is the owner, and v1 is built for them.
- **Secondary (after the audit):** anyone who wants a no-subscription password manager on storage they
  control, installed from GitHub, Homebrew, the app stores or the web.

## 4. Goals and success measures

| Goal                                   | Measure                                                                 |
| -------------------------------------- | ----------------------------------------------------------------------- |
| Replace the notes app for secrets      | All existing notes imported; any item found by a fuzzy query in < 100 ms |
| Safe agent use of tokens               | A Claude Code session deploys and calls APIs with no token in the transcript |
| Works on every device the owner uses   | Mac, iPhone and a browser sync the same vault through iCloud Drive or Google Drive |
| Trustworthy for others                 | External audit passed and findings published before the public release   |
| Zero running cost per user             | No FigLock server stores user data; optional relay stores nothing        |
| Fast onboarding                        | A new user is set up with their own Google Drive in under 3 minutes      |

## 5. Non-goals

- No hosted accounts, databases or servers that store vault data.
- No team or enterprise admin (SSO, SCIM, policies).
- No server-side search.
- Not a secrets manager for production infrastructure.
- Not a sandbox against a fully malicious agent with shell access; we reduce leaks and blast radius and
  say exactly what is protected.

## 6. User stories

- As the owner, I import my notes dump and FigLock detects `ghp_...`, `sk-ant-...` and `AKIA...` tokens
  and files them as API token items.
- As the owner, I type `figlock find clodflare` and get my Cloudflare tokens despite the typo.
- As the owner, I keep three logins for the same site and see them grouped under that site.
- As the owner, I unlock with Touch ID and the vault locks again on sleep or idle.
- As the owner, when Claude Code needs my GitHub token it asks the broker, I approve once with Touch ID
  for "this session", and the token is added to the request without showing up in the transcript.
- As the owner, I approve a laptop agent request from my iPhone (with the optional relay).
- As the owner, I add a new device by scanning or typing a short code on a device I already trust.
- As the owner, I lose my laptop and recover from my Emergency Kit on a new one.
- As a new user, I install FigLock, pick Google Drive, sign in once, and I'm done.
- As any user, I export everything, encrypted or plain, at any time.

## 7. Functional requirements

Priority: **must** = MVP (phases 1-2), **should** = v1 (phases 3-5), **later** = after v1 (phase 6+).

### 7.1 Items and data

| Requirement                                                                 | Priority |
| --------------------------------------------------------------------------- | -------- |
| Login: title, many URLs, username, password, notes, custom fields           | must     |
| API token: value, provider, env var name, scopes, expiry, allowed hosts      | must     |
| Tags and multiple vaults (Personal, Work)                                    | must     |
| Password and passphrase generator                                            | must     |
| Import: Chrome, Apple, Bitwarden, 1Password CSV; raw notes dump with token detection | must |
| Export, encrypted and plain                                                  | must     |
| Secure note, SSH key, `.env` bundle                                          | should   |
| TOTP next to the login (opt-in per item)                                     | should   |
| Item history (previous values)                                               | should   |
| FIDO CXF import/export, passkey storage                                      | later    |

### 7.2 Search

| Requirement                                                                 | Priority |
| --------------------------------------------------------------------------- | -------- |
| Local fuzzy, typo-tolerant search over every non-secret field                 | must     |
| Search by domain, token prefix and recency                                   | must     |
| Aliases ("cf" finds Cloudflare)                                              | should   |

### 7.3 Security

| Requirement                                                                 | Priority |
| --------------------------------------------------------------------------- | -------- |
| Two-secret key derivation (master password + Secret Key)                     | must     |
| Biometric quick unlock (Touch ID, Face ID, Windows Hello, Android)           | must     |
| Auto-lock on idle, sleep and screen lock                                     | must     |
| Clipboard auto-clear (30 s), copy without history                            | must     |
| Emergency Kit; optional Shamir recovery                                      | must     |
| Encrypted audit log of every agent access                                    | must     |
| Token expiry and rotation reminders                                          | should   |
| Weak, reused and breached password report (HIBP k-anonymity)                 | should   |

### 7.4 Sync and storage

| Requirement                                                                 | Priority |
| --------------------------------------------------------------------------- | -------- |
| Local / synced folder storage (iCloud Drive on Mac)                          | must     |
| Conflict-free multi-device sync via per-device op logs and CRDT merge         | must     |
| Device pairing through `inbox/`, device revocation                           | must     |
| Google Drive appDataFolder storage (`drive.appdata` scope only)              | should   |
| Git and S3 storage                                                           | later    |
| Sharing with another FigLock user                                            | later    |

### 7.5 Agent access

| Requirement                                                                 | Priority |
| --------------------------------------------------------------------------- | -------- |
| Broker daemon with grants (who, what, mode, how long) approved by biometrics | must     |
| `figlock http` with per-token host allow-lists                               | must     |
| `figlock run` env injection, `--bundle`, output scrubbing                    | must     |
| Claude skill shipped in the CLI (`figlock skill install`), `--json` on every command | must |
| `figlock reveal`, opt-in per item, own approval each time                    | should   |
| Approve from iPhone via optional stateless relay                             | should   |
| MCP adapter for shell-less clients                                           | later    |

### 7.6 Platforms

| Requirement                                                                 | Priority |
| --------------------------------------------------------------------------- | -------- |
| CLI: macOS, Linux, Windows                                                   | must     |
| Desktop app (Tauri 2), macOS first                                           | should   |
| Web PWA at app.figlock.com                                                   | should   |
| Browser extension with autofill (Chrome, Firefox, Safari, Edge)              | should   |
| iOS app with AutoFill credential provider                                    | should   |
| Android app with AutofillService                                             | should   |

## 8. Non-functional requirements

- **Security:** only standard primitives, spec with test vectors, fuzzing, external audit before
  inviting other users. Secrets zeroized in memory. Ops signed by device keys.
- **Privacy:** storage sees only ciphertext, counts, sizes and timestamps. No telemetry by default.
- **Offline:** every device keeps a full encrypted local copy and works offline.
- **Performance:** unlock < 1 s on a modern laptop (Argon2id tuned per device class); search < 100 ms for
  10,000 items; sync of a new change visible on other devices within 30 s while open.
- **Portability:** one Rust core on every platform (native, WASM, UniFFI).
- **Supply chain:** reproducible, signed releases; cargo-deny and cargo-vet in CI.
- **Accessibility:** apps meet platform accessibility guidelines; web meets WCAG 2.2 AA.

## 9. Release plan

| Phase | Scope                                   | Exit test                                                                 |
| ----- | --------------------------------------- | ------------------------------------------------------------------------- |
| Setup | Repo, tracker, design tool, local dev   | Repo public with CI green; Paperclip seeded; AGENTS.md in place; OpenDesign connected |
| 0     | Specs, threat model, ADRs, design system | Specs merged with test vectors; DESIGN.md and key screen prototypes approved |
| 1     | Core + CLI + local folder storage       | Notes imported and found by fuzzy search on two Macs via iCloud Drive      |
| 2     | Broker + skill                          | Claude Code deploys and calls GitHub with no token in the transcript       |
| 3     | Desktop app + Google Drive              | Mac and Windows/Linux sync through Google Drive; pairing under a minute    |
| 4     | PWA + browser extension                 | Log in to 20 everyday sites from the extension                            |
| 5     | iOS, then Android                       | iOS autofill works; agent grant approved from iPhone; Android at parity    |
| 6     | Public release                          | Audit fixed and published; new user set up in under 3 minutes             |

## 10. Risks

| Risk                                                   | Mitigation                                                          |
| ------------------------------------------------------ | ------------------------------------------------------------------- |
| User loses password and Secret Key: data gone forever  | Emergency Kit with forced "verify you saved it" step; Shamir; any paired device can re-issue access |
| Subtle bugs in our own protocol                        | Standard primitives, spec + test vectors, fuzzing, external audit   |
| Web app trust (whoever serves the JS can change it)    | Open source, immutable deploys, CSP, visible build hash; push power users to extension/desktop |
| appDataFolder deleted when user disconnects the app    | Full local copy on every device; re-upload                          |
| Autofill is platform-specific and hard                 | Native shells; ship CLI and desktop value first                     |
| Agents with shell access misuse an injected secret     | Default to broker HTTP with allow-lists, short TTLs, audit log, loud reveal |

## 11. Open questions

- Trademark and App Store / Play Store availability of "FigLock".
- Individual or company Apple Developer account (a company needs a D-U-N-S number).
- Windows signing: Azure Artifact Signing or a classic certificate.
- Audit firm and budget.
