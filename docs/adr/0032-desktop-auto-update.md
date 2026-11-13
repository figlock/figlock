# ADR-0032: The desktop app updates itself from a signed feed, on the person's click

- Status: Accepted
- Date: 2026-11-14
- Decision: D32 in `docs/decisions.md`

## Context

The desktop app ships as a signed, notarized dmg with the `figlock` CLI inside it (FIG-110, FIG-111). People need
a way to get new versions without finding and reinstalling the dmg, and the CLI inside must always be the same
version as the app. FigLock has no server of ours (AGENTS.md), so the update feed has to live somewhere that
already exists.

## Decision

- **Tauri's updater, with our own key.** The app checks a manifest, downloads the archive it names and verifies a
  minisign signature against the public key in its config before it installs anything. The key is generated once
  by the maintainer (`tauri signer generate`), kept in the `release` environment as `TAURI_SIGNING_PRIVATE_KEY`
  and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and backed up. The Apple certificate does not take its place:
  macOS trusts the certificate, the updater trusts this key, and a compromise of one is not a compromise of the
  other.
- **The archive is the whole signed app.** It is made from the app after it was signed, notarized and stapled
  (`scripts/package-update.sh`), so an update replaces the app and the CLI inside it together and the installed
  bundle keeps a valid Developer ID signature. It carries no macOS metadata entries, which the updater cannot
  unpack (found by testing).
- **The signature is bound to the version.** The release signs with `--app-version` and the app sets
  `requireSignedVersion`, so a tampered manifest cannot pair a new version number with an older signed archive.
  An update that is not strictly newer than the running version is not offered.
- **The feed is a GitHub release asset behind figlock.com.** The manifest is `latest.json` on the latest release,
  and the app reads it from `https://figlock.com/update/latest.json`, which `site/_redirects` sends to the
  release; archive URLs are `https://figlock.com/download/v<version>/...`. A move of the repository or the
  host changes the redirect and nothing in installed apps. No new server, account or database.
- **Check at launch and from Settings; install only on a click.** The check at launch is quiet: it shows a
  notice in the sidebar only when there is a newer version. Installing needs a click and downloads, verifies and
  installs; the person then restarts (with a confirmation, because the vault locks). The request carries nothing
  from the vault and Settings says so.
- **Failures leave the installed app as it was.** A bad signature, a version mismatch, a failed download or a
  failed install show plain words ("could not be verified, so it was not installed; your current version is
  unchanged") and the real cause goes to the log. After a failed install the app asks the feed again.
- **A build without an update key cannot update and says so.** Development builds have no `plugins.updater`
  config; the plugin is not started and Settings says "this build does not update itself". A release build
  must have a well-formed key and an https feed: `scripts/check-update-config.sh` fails the release workflow
  otherwise.
- **The window gets no updater permissions.** It calls three commands of the app (`update_check`,
  `update_install`, `restart_app`); the capability file is unchanged.

## Consequences

- Losing the update key or its password means no installed app can take another update; people would reinstall
  from the dmg. The key is backed up for that reason. Rotating it means shipping one release signed with the old
  key that carries the new public key.
- The first release with the updater cannot update anyone; the second can. The update mechanics were tested
  locally end to end with a throwaway key and a local feed (bad signature, version mismatch, a failed install and
  a good install and restart).
- Windows and Linux (FIG-113) use the same feed and key with their own archives.
- No change to the crypto, the vault format or the device list.
