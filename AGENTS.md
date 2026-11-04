# AGENTS.md

Standing instructions for any agent (or human) working on FigLock. Read this before picking up a task or
writing code. `docs/prd.md` says what we are building, `docs/decisions.md` says what is already decided,
and `docs/research.md` holds the findings behind those decisions.

## What this is

FigLock is a headless, agent-aware password manager: one encrypted vault for passwords, API tokens and
secrets, usable from CLI, desktop, web, browser extension and native mobile apps, with no backend or
database of our own. The vault lives as ciphertext in storage the user already owns (Google Drive
appDataFolder, iCloud Drive or another synced folder, later Git and S3). AI agents can *use* a secret after
the user approves with Touch ID, ideally without ever seeing it.

## Non-negotiables

- **No secret ever leaves the vault in plain text by accident.** Never put real secrets, tokens, recovery
  codes or local file paths in tasks, GitHub issues, PR text, commit messages, logs, test fixtures or
  screenshots. Tests use generated values and the published test vectors in `docs/spec/`.
- **Only standard primitives.** Argon2id, HKDF, XChaCha20-Poly1305, X25519, Ed25519 from RustCrypto (or
  platform APIs). No hand-rolled crypto. Any change to the crypto or sync spec needs an ADR and new test
  vectors before code.
- **Ciphertext only on storage.** Item titles, URLs and usernames are encrypted too. The storage provider
  may see file counts, sizes and timestamps, nothing else.
- **No server of ours holds vault data.** The optional relay forwards end-to-end encrypted approvals and
  stores nothing. Don't add a backend, account system or database to make something easier.
- **Agents use secrets, they don't hold them.** Default to broker HTTP with host allow-lists, then env
  injection with output scrubbing. Reveal is opt-in per item and needs its own approval every time.

## How we track work (Paperclip + GitHub)

- **Paperclip is the plan.** It runs locally at `http://localhost:3100` and holds the goal, one project per
  phase, milestones, tasks, blockers ("blocks" links), the assets register and these docs. If something is
  not in Paperclip, it is not planned.
- **GitHub is the public record.** Repo `seemantshekhar43/figlock` (public) for now; it moves to a
  `figlock` org later and GitHub redirects the old URLs. A task gets a GitHub issue the moment work starts
  on it, not before. Never hard-code the github.com path in user-facing places: use figlock.com URLs
  (`figlock.com/install.sh`, docs links) so the move is invisible to users.
- **Start-a-task routine** (do every step, in order):
  1. Move the Paperclip task to **In Progress**.
  2. `gh issue create --repo seemantshekhar43/figlock --title "FIG-<n>: <task title>" --label <area>` with the
     task's goal and exit criteria as the body. No secrets, no local paths.
  3. Comment the GitHub issue link on the Paperclip task.
  4. Branch `FIG-<n>-<short-slug>` off `main`.
  5. Open a PR whose title starts with `FIG-<n>` and whose body says `Closes #<github-issue>`.
  6. When the PR merges, move the Paperclip task to **Done** and link the PR in a comment.
- **Blocked?** Move the task to **Blocked**, add a "blocked by" link to the blocking task (create one if it
  doesn't exist), and say in a comment what would unblock it.
- **Nothing moves to Done without review.** Agents stop at **In Review**; the human approves.
- **Decisions** that change direction get an ADR in `docs/adr/` and a line in `docs/decisions.md`.

## Working style

- Prefer quality, simplicity, robustness and long-term maintainability over development speed.
- For one-off or infrequent operational work, take the simplest direct path. Don't build wrappers,
  control planes or automation until a concrete blocker or repeated need justifies it.
- Bug fixes start by reproducing the bug end to end, as close to how a user hits it as possible.
- Be picky about UI: if something looks off, even outside the current task, fix it or file a task.
- Lint errors, test failures and flaky tests are always worth fixing, even when you didn't cause them.
- Plans, comparisons and reports for human review go through `inkloop` (`npx -y inkloop <file.html>`),
  kept under `.inkloop/`.
- Use `gh-axi` for GitHub operations when available.
- Ask before using any harness feature that spawns a large swarm of subagents.
- Writing: never use the em dash character; use a plain dash `-`.
- Commits: conventional commits (`feat(core): ...`, `fix(cli): ...`). Never add an agent as co-author.
- Never hand-edit `CHANGELOG.md` or any generated file; release notes come from the release tool.

## UI design (OpenDesign)

- Design happens in OpenDesign (local desktop app, connected to Claude Code with `od mcp install claude`).
- `design/DESIGN.md` and `design/tokens.css` in this repo are the brand contract: colors, type, spacing,
  radius, motion, voice. OpenDesign reads them; every app builds from them.
- Prototypes (HTML) are references, not shipped code. Web, desktop and extension implement them in
  `packages/ui` (Svelte 5 + Tailwind using the tokens); iOS and Android map the same tokens to SwiftUI and
  Compose themes.
- Export prototypes and screenshots into `design/prototypes/` so reviews and tasks can link to them.
- Never put real vault data, tokens or personal details into OpenDesign; it sends some telemetry and may
  call cloud models. Use made-up sample items.

## Repo layout (monorepo)

```
crates/figlock-core/     crypto, items, op log, merge, search (pure, no I/O)
crates/figlock-store/    BlobStore adapters: local folder, Google Drive, later Git and S3
crates/figlock-cli/      the figlock binary, including the broker daemon
crates/figlock-ffi/      UniFFI (Swift, Kotlin) and wasm-bindgen exports
apps/desktop/            Tauri 2
apps/web/                PWA (app.figlock.com)
apps/extension/          WXT browser extension
apps/ios/  apps/android/ native shells
packages/ui/             shared Svelte 5 components
relay/                   optional stateless Cloudflare Worker (relay.figlock.com)
site/                    figlock.com static pages
skill/                   SKILL.md shipped inside the CLI
docs/spec/  docs/adr/    specs, test vectors and decision records
design/                  DESIGN.md, tokens, exported prototypes (OpenDesign)
```

Separate repos only where a tool requires it: `homebrew-tap` (Homebrew) and, once the org exists,
`.github` (org profile and default community files).

## Testing philosophy

- Every crypto and merge rule has test vectors in `docs/spec/` and a test that checks them.
- Convergence of the op-log merge is proven with property tests (proptest), not examples alone.
- Parsers and importers are fuzzed (cargo-fuzz).
- Anything security-relevant gets an explicit negative test: wrong key, tampered op, revoked device,
  host not on the allow-list, secret value appearing in scrubbed output.
- UI flows get Playwright end-to-end tests; mobile gets UI tests on simulators.
