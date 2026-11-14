# Releasing figlock

A release is a version tag. `release.yml` does the rest and the `release` environment holds the secrets
(setup in `docs/macos-signing.md`).

Nothing reaches a user until the release has been staged as a draft and verified. `latest.json` on the latest
release is what every installed app trusts for updates, so a broken release would break updating for everyone.

## Cut a release

1. Bump `version` under `[workspace.package]` in `Cargo.toml` (and the path-dependency `version` fields in
   `crates/*/Cargo.toml`), merge it to `main` through a PR.
2. Tag the merge commit and push the tag: `git tag v<version> && git push origin v<version>`.
3. Approve the `release` environment when the run asks: once for each of the Linux builds, once for the macOS
   signing job, and once for `publish`. The approval for `publish` only comes after verification has passed.
4. The run goes through these jobs:
   - `check-version`: the tag must match the workspace version. A version with a suffix (`0.3.0-rc1`) is a
     pre-release, see below.
   - `linux` and `macos`: build and sign everything.
   - `stage`: packages the files, writes the release notes (GitHub's generated notes for the release page; the
     manifest's "What is new" gets the same list as plain `- title (#123)` lines, without the links), and uploads it all to a **draft** release. A draft is private.
   - `verify`: downloads the draft's assets as they were uploaded and runs `scripts/verify-release.sh`.
   - `publish`: makes the draft public as the latest release, checks the production feed, and updates the Homebrew
     tap.
5. The release has:
   - `figlock-<version>-universal-apple-darwin.tar.gz` (signed, notarized)
   - `FigLock-<version>-universal.dmg`, the desktop app with the CLI inside (signed, notarized, stapled)
   - `FigLock-<version>-universal.app.tar.gz`, its `.sig` and `latest.json`: what the in-app updater reads
   - `figlock-<version>-x86_64-unknown-linux-musl.tar.gz` and `figlock-<version>-aarch64-unknown-linux-musl.tar.gz`
   - `SHA256SUMS`
   - `figlock.rb`, the Homebrew formula with the version and checksums filled in

The run fails before building if the tag does not match the workspace version, and a missing signing or
build secret fails the first step of that job. The desktop app takes its version from the workspace too
(`tauri.conf.json` deliberately has none), and the macOS job fails if the built app is not that version
(`scripts/check-app-version.sh`) or if the CLI inside it reports a different one (`scripts/verify-macos-app.sh`).

## What the verification checks

`scripts/verify-release.sh <asset-dir> <tag>` fails the run, leaving the draft in place, unless:

- every asset above is present and not empty;
- `SHA256SUMS` lists the three CLI archives and the dmg, every checksum matches its file, and `install.sh` finds
  exactly one archive for each target;
- `latest.json` is valid JSON whose `version` is the tag, with notes, with both `darwin-aarch64` and
  `darwin-x86_64`, each pointing at `https://figlock.com/download/<tag>/<archive>` and carrying the signature
  that is in the `.sig` file;
- that signature verifies against the public key in `tauri.bundle.conf.json` (the key every installed app trusts),
  and its trusted comment names this archive and this version (the apps set `requireSignedVersion`);
- the archive has no macOS metadata (`._*`) entries and nothing outside `FigLock.app`;
- `figlock.rb` is filled in for this version.

`scripts/test-release-scripts.sh` proves each of these: a good release passes and every check is broken in turn
and must fail for its own reason. CI runs it whenever the scripts, the release workflows or the update key change
(`just release-scripts-test`, needs `minisign`).

A failed `verify` leaves a draft release behind. Look at the log, fix the cause on `main`, delete the draft
(`gh release delete <tag> --cleanup-tag --yes`; `--cleanup-tag` removes the tag too, so it can be pushed again
on the fixed commit), and tag again.

## Pre-releases

A tag with a suffix, for example `v0.3.0-rc1`, is published as a **pre-release**. GitHub never counts a pre-release
as latest, so `releases/latest`, `install.sh` and the updater (through `figlock.com/update/latest.json`) do not see
it, and the Homebrew tap is not touched. The run checks that the production feed does not serve it. Install one
with `FIGLOCK_VERSION=v0.3.0-rc1` for `install.sh`, or download the dmg from the release page.

## Roll back a bad release

The updater reads `latest.json` from the latest release, so to stop offering a bad release, make it not the latest:

1. `gh release edit v<bad> --prerelease --latest=false` (or `gh release delete v<bad>` for good). GitHub then
   serves the previous stable release as latest again, and `https://figlock.com/update/latest.json` follows within
   a minute. Check it: `curl -fsSL https://figlock.com/update/latest.json | python3 -m json.tool | head`.
2. The Homebrew tap still points at the bad version: revert the last commit of `Formula/figlock.rb` in
   `figlock/homebrew-tap`.
3. Apps that already updated to the bad version stay on it: an update must be strictly newer than the running
   version, so the way out is a fixed release with a higher number. Ship `v<bad+1>` through this same flow.
4. Do not reuse or move a published tag.

## The in-app updater (ADR-0032)

The desktop app checks `https://figlock.com/update/latest.json` at launch and from Settings, and installs only when
the person clicks. `site/_redirects` sends that URL to `latest.json` on the latest GitHub release and
`/download/*` to the release's assets, so only that file knows where the release lives.

- **The update key** is separate from the Apple certificate. It is already generated and its public half is in the bundle
  config; to rotate it (which strands installed apps, see ADR-0032), generate a new one on your machine:
  `cd apps/desktop && pnpm tauri signer generate -w ~/Documents/apple-creds/updater/figlock-updater.key`, set the secrets
  `TAURI_SIGNING_PRIVATE_KEY` (the key file) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the `release`
  environment, and replace the public key (`.key.pub`) in `apps/desktop/src-tauri/tauri.bundle.conf.json` under
  `plugins.updater.pubkey`. Back the key and password up: without them no installed app can take another update.
- **A release build must have the key and feed.** `scripts/check-update-config.sh` is the first check of the macOS
  workflow and fails it when `plugins.updater` is missing, the endpoint is not https, the key is malformed or
  `requireSignedVersion` is off.
- **The first release with the updater cannot update anyone; the second can.** Cut a second release (even a
  patch) and update an installed copy of the first to confirm production works end to end, as below.
- Try the whole flow locally, without Apple signing: see "Trying updates locally" in `docs/dev-setup.md`.

## Test updates in production

After two real releases, for example `v0.3.0` and then a small real `v0.3.1`:

1. On a clean Mac or a fresh user account, install `FigLock-0.3.0-universal.dmg`. Gatekeeper opens it without a
   warning, `figlock --version` says 0.3.0 (Settings > Command line tool, if installed), Settings > Updates says
   the app is up to date, and `https://figlock.com/update/latest.json` returns the manifest.
2. Publish `v0.3.1`. In the installed app the sidebar shows Update available, Install update downloads and verifies
   it through figlock.com, Restart opens 0.3.1, the CLI inside reports 0.3.1, the vault still unlocks, and the
   command line tool link still works.
3. Negative checks on production: the app on the latest version says it is up to date, and
   `https://figlock.com/download/v9.9.9/FigLock-9.9.9-universal.app.tar.gz` fails cleanly (404, not a manifest).
4. Record the result, with the run links, on the release task.

## Install paths

- `curl -fsSL https://figlock.com/install.sh | sh` picks the archive for the machine, checks it against
  `SHA256SUMS` and installs to `~/.local/bin` (`FIGLOCK_INSTALL_DIR`, `FIGLOCK_VERSION` and `FIGLOCK_BASE_URL`
  override). Source: `site/install.sh`. The extra release assets (dmg, update archive, manifest) do not confuse
  it: it only reads the archive lines of `SHA256SUMS`, and `verify-release.sh` checks there is exactly one per target.
- Homebrew: `brew install figlock/tap/figlock`. The `publish` job writes `Formula/figlock.rb` into
  `figlock/homebrew-tap` with the `HOMEBREW_TAP_TOKEN` secret (a fine-grained token with Contents read and write
  on that one repo, kept in the `release` environment; it expires after a year, so renew it or releases fail at
  the last step). Pre-releases do not update it.
- crates.io is deliberately not used (ADR-0021: a source build has no Google client secret, so Drive sign-in
  would not work).

## Try the pieces locally

```sh
scripts/package-release.sh 9.9.9 <dir-with-one-folder-per-target> out
FIGLOCK_BASE_URL=http://127.0.0.1:8765 FIGLOCK_INSTALL_DIR=/tmp/fl sh site/install.sh   # with out/ served there
scripts/test-release-scripts.sh                                                           # needs minisign
```
