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

### The app with the command line tool inside

`scripts/build-desktop.sh [target]` builds the `figlock` CLI from the same commit, puts it where Tauri expects a
sidecar and bundles the app, so `FigLock.app/Contents/MacOS/figlock` is the tool. Use `universal-apple-darwin`
for a universal macOS build. The bundle settings live in `apps/desktop/src-tauri/tauri.bundle.conf.json` and are
merged only by that script, so `pnpm tauri dev`, tests and lint need no sidecar binary. In a build without the
tool, Settings says so; in one with it, Settings offers an opt-in "Install command line tool" that links it at
`~/.local/bin/figlock` and never changes PATH. If a `figlock` from another install (Homebrew, the install
script) is already on this Mac, Settings names it and adds no second copy. The output is unsigned; signing and the dmg are the release
workflow's job.

### Trying updates locally

The in-app updater (ADR-0032) can be tried end to end with a throwaway key and a local feed; nothing here touches
the release key or Apple signing. Build two versions of the app, serve the newer one, and update the older one.

```sh
cd apps/desktop
pnpm tauri signer generate --ci -p "" -w /tmp/updtest.key           # throwaway key, not the release key
cat > /tmp/updtest.conf.json <<EOF
{"plugins":{"updater":{"pubkey":"$(cat /tmp/updtest.key.pub)","endpoints":["http://127.0.0.1:8766/latest.json"],
  "dangerousInsecureTransportProtocol":true,"requireSignedVersion":true}}}
EOF
T=aarch64-apple-darwin; C="--config src-tauri/tauri.bundle.conf.json --config /tmp/updtest.conf.json"
../../scripts/build-desktop.sh $T >/dev/null 2>&1 || true            # puts the CLI where Tauri wants it
pnpm tauri build --target $T $C && cp -R ../../target/$T/release/bundle/macos/FigLock.app /tmp/installed.app
pnpm tauri build --target $T $C --config '{"version":"0.2.1"}'       # a newer one
codesign --force --deep -s - ../../target/$T/release/bundle/macos/FigLock.app
TAURI_SIGNING_PRIVATE_KEY_PATH=/tmp/updtest.key TAURI_SIGNING_PRIVATE_KEY_PASSWORD= FIGLOCK_UPDATE_ALLOW_SINGLE_ARCH=1 \
  FIGLOCK_UPDATE_BASE_URL=http://127.0.0.1:8766/download \
  ../../scripts/package-update.sh ../../target/$T/release/bundle/macos/FigLock.app /tmp/feed/download/v0.2.1
cp /tmp/feed/download/v0.2.1/latest.json /tmp/feed/ && (cd /tmp/feed && python3 -m http.server 8766 --bind 127.0.0.1)
```

Run `/tmp/installed.app` with `FIGLOCK_DEVICE_KEYS=file` and a throwaway `FIGLOCK_HOME`, unlock, and the sidebar
shows "Update available". Change the version in `latest.json` to see a mismatch refused, or corrupt the signature
to see "could not be verified". The real cause of any failure is printed to the app's stderr.

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
