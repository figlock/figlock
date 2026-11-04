# Dev setup

From a clean Mac to a passing `just check`.

## 1. Prerequisites

- Xcode command line tools: `xcode-select --install`
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
git clone https://github.com/seemantshekhar43/figlock.git
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
| `just check` | `lint` then `test` (what CI runs) |

Recipes skip a language until its workspace exists (`Cargo.toml`, `pnpm-workspace.yaml`).

## Versions

Pinned in [`mise.toml`](../mise.toml). Bump them there, in one PR, and nowhere else.
