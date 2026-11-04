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

## Starting and ending a session

Chat history does not carry over; Paperclip does. At the start of every session, before anything else:

1. Make sure Paperclip is up (`curl -s http://127.0.0.1:3100/api/health`, otherwise `paperclipai run`).
2. **In Progress** tasks: read their latest comments and linked PR, and offer to resume them.
3. **In Review** tasks: list them as waiting on the human.
4. Suggest the next 2-3 **Todo** tasks with no open blockers, highest priority first. Start one only
   when the human picks it (short prompts like "what's next?", "do FIG-9" or "continue" mean exactly
   this).

Before a session ends with a task unfinished, leave a handoff comment on that Paperclip task: what is
done, what is left, the branch or PR, and anything the next session must not redo. The next session
starts from that comment.

## How we track work (Paperclip + GitHub)

- **Paperclip is the plan.** It runs locally at `http://127.0.0.1:3100` and holds the goal, one project per
  phase, tasks, blockers ("blocked by" links), the assets register and the project handbook (FIG-61). If
  something is not in Paperclip, it is not planned.
- **GitHub is the public record of code.** Repo `seemantshekhar43/figlock` (public) for now; it moves to a
  `figlock` org later and GitHub redirects the old URLs. Never hard-code the github.com path in user-facing
  places: use figlock.com URLs (`figlock.com/install.sh`, docs links) so the move is invisible to users.

### Where things go

| What | Where |
| --- | --- |
| Tasks, status, blockers, discussion | Paperclip task and its comments |
| Local working artifacts: inkloop plans, comparisons, research notes, screenshots, OpenDesign exports under review | Paperclip, on the task they belong to. Markdown as a task **document** (versioned, `PUT /api/issues/{id}/documents/{key}`), any other file as an **attachment** (`POST /api/companies/{companyId}/issues/{id}/attachments`, multipart field `file`). Not committed, not on GitHub. |
| Things the code or users depend on: PRD, decisions, ADRs, specs, test vectors, `design/DESIGN.md` | The repo. Refresh the copy on FIG-61 when they change. |
| Work that lands as a commit or PR | A GitHub issue too (routine below) |

Research, planning, account and DNS chores, and other tasks with no code change stay in Paperclip only.
Paperclip data lives in `~/.paperclip`, outside git, so back it up with `paperclipai db:backup` before
anything risky.

### Using Paperclip from a new session

- Start it if it isn't running: `curl -s http://127.0.0.1:3100/api/health`, otherwise `paperclipai run`.
- It runs in `local_trusted` mode, so the API on loopback needs no token. Company prefix `FIG`; look up the
  company id with `GET /api/companies`.
- Don't guess the API. Paperclip describes itself:
  - `GET /api/openapi.json`: every route.
  - `GET /api/skills/paperclip`: Paperclip's own agent guide (documents, attachments, work products,
    blockers, approvals). It is written for agents that Paperclip wakes on a heartbeat, so skip the
    heartbeat, checkout and run-token parts when working interactively.
  - `paperclipai --help`, `paperclipai issue --help`: the CLI covers issues, comments, documents and work
    products.
- After an upgrade (`paperclipai update`), re-read those instead of trusting this file's route names.

### Start-a-task routine

For a task that will change code, do every step, in order:

1. Move the Paperclip task to **In Progress**.
2. `gh issue create --repo seemantshekhar43/figlock --title "FIG-<n>: <task title>" --label <area>` with the
   task's goal and exit criteria as the body. No secrets, no local paths.
3. Comment the GitHub issue link on the Paperclip task.
4. Branch `FIG-<n>-<short-slug>` off `main`.
5. Open a PR whose title starts with `FIG-<n>` and whose body says `Closes #<github-issue>`.
6. When the PR merges, link the PR on the Paperclip task and move it to **In Review** for the human.

A task with no code change only needs step 1, its artifacts attached in Paperclip, and **In Review** at
the end.

### Rules
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
  kept under `.inkloop/` (gitignored). Once a review round ends, attach the HTML to its Paperclip task.
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
