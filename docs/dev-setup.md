# Dev setup

From a clean Mac to a passing `just check`.

## 1. Prerequisites

- Xcode command line tools: `xcode-select --install`. They include `swiftc`, which builds the small Swift shim
  for Secure Enclave quick unlock (`crates/figlock-biometric/swift`). Without it the build still works and
  quick unlock keeps using the keychain, with a build warning.
- [Homebrew](https://brew.sh)
- Git, and the GitHub CLI if you open PRs: `brew install git gh`

## 2. Install mise

[mise](https://mise.jdx.dev) pins every toolchain so everyone builds with the same versions.

```sh
brew install mise
echo 'eval "$(mise activate zsh)"' >> ~/.zshrc   # then open a new shell
```

## 3. Clone and set up

```sh
git clone https://github.com/figlock/figlock.git
cd figlock
mise trust
mise install                  # Rust (with rustfmt, clippy), Node, pnpm, just, cargo-deny
git config core.hooksPath .githooks   # pre-commit hook: format check and lint
```

`just setup` does the last two steps for you once `just` is on your path.

## 4. Everyday commands

| Command | What it does |
| --- | --- |
| `just fmt` | Format Rust and JS/TS code |
| `just lint` | Format check plus clippy with warnings as errors, and JS lint |
| `just test` | Run all tests |
| `just deny` | cargo-deny: advisories, licenses, bans, sources |
| `just check` | `lint` then `test` (CI also runs `deny`) |

Recipes skip a language until its workspace exists (`Cargo.toml`, `pnpm-workspace.yaml`).

## Desktop app

`apps/desktop` is the Tauri 2 shell and `packages/ui` the shared Svelte 5 components. After `mise install`:

```sh
pnpm install
cd apps/desktop && pnpm tauri dev    # window with hot reload
```

`just check` builds and lints the Tauri crate with the rest of the workspace; the first run compiles Tauri and
takes a few minutes. On Linux, install the WebKitGTK build libraries first (the same list CI uses):
`libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libayatana-appindicator3-dev`.

## Google Drive sign-in in a local build

Source builds have no Google client secret (ADR-0021). To try `figlock drive login` or Google Drive in the desktop app locally, set it for the run
(never commit it, never paste it into an issue or PR):

```sh
read -rs FIGLOCK_GOOGLE_CLIENT_SECRET && export FIGLOCK_GOOGLE_CLIENT_SECRET
```

The secret is in the owner's FigLock vault. While the Google consent screen is in Testing status, only listed
test accounts can sign in. Official release builds get the secret from CI (`FIGLOCK_BUILD_GOOGLE_CLIENT_SECRET`).

## Versions

Pinned in [`mise.toml`](../mise.toml). Bump them there, in one PR, and nowhere else.
