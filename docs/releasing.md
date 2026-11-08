# Releasing figlock

A release is a version tag. `release.yml` does the rest and the `release` environment holds the secrets
(setup in `docs/macos-signing.md`).

## Cut a release

1. Bump `version` under `[workspace.package]` in `Cargo.toml` (and the path-dependency `version` fields in
   `crates/*/Cargo.toml`), merge it to `main` through a PR.
2. Tag the merge commit and push the tag: `git tag v<version> && git push origin v<version>`.
3. Approve the `release` environment when the run asks (once for each of the Linux builds, the macOS signing
   job and the publish job).
4. The `publish` job creates the GitHub release with:
   - `figlock-<version>-universal-apple-darwin.tar.gz` (signed, notarized)
   - `figlock-<version>-x86_64-unknown-linux-musl.tar.gz` and `figlock-<version>-aarch64-unknown-linux-musl.tar.gz`
   - `SHA256SUMS`
   - `figlock.rb`, the Homebrew formula with the version and checksums filled in

The run fails before building if the tag does not match the workspace version, and a missing signing or
build secret fails the first step of that job.

## Install paths

- `curl -fsSL https://figlock.com/install.sh | sh` picks the archive for the machine, checks it against
  `SHA256SUMS` and installs to `~/.local/bin` (`FIGLOCK_INSTALL_DIR`, `FIGLOCK_VERSION` and `FIGLOCK_BASE_URL`
  override). Source: `site/install.sh`.
- Homebrew: `brew install figlock/tap/figlock`. The `publish` job writes `Formula/figlock.rb` into
  `figlock/homebrew-tap` with the `HOMEBREW_TAP_TOKEN` secret (a fine-grained token with Contents read and write
  on that one repo, kept in the `release` environment; it expires after a year, so renew it or releases fail at
  the last step).
- crates.io is deliberately not used (ADR-0021: a source build has no Google client secret, so Drive sign-in
  would not work).

## Try the pieces locally

```sh
scripts/package-release.sh 9.9.9 <dir-with-one-folder-per-target> out
FIGLOCK_BASE_URL=http://127.0.0.1:8765 FIGLOCK_INSTALL_DIR=/tmp/fl sh site/install.sh   # with out/ served there
```
