# Signing and notarizing the macOS CLI and desktop app

The `Release (macOS)` workflow builds a universal `figlock` binary, signs it with a Developer ID Application
certificate (hardened runtime), notarizes it with Apple and uploads `figlock.zip` as a build artifact.
`release.yml` calls it on a version tag and publishes the result (`docs/releasing.md`). `scripts/sign-macos.sh` does the signing and notarizing and also
works on a developer Mac.

The same workflow then builds the desktop app (`scripts/build-desktop.sh universal-apple-darwin`, which puts the
same CLI inside it, FIG-110), signs and notarizes it and packs it into `FigLock-<version>-universal.dmg`
(`scripts/sign-macos-app.sh`, FIG-111). See "The desktop app and its dmg" below.

A bare command-line binary cannot be stapled. Gatekeeper looks up the notarization ticket online the first
time a quarantined download runs, so distribute the signed zip (or an archive containing the binary).
The binary needs no entitlements: hardened runtime, the login keychain and Touch ID via LocalAuthentication
all work without any.

## One-time setup (account holder, not an agent)

Never put the certificate, the key or any password in git, issues, logs or screenshots.

1. **Certificate.** In Keychain Access, Certificate Assistant, request a certificate from a CA and save the
   CSR. At developer.apple.com, Certificates, create a **Developer ID Application** certificate from that CSR,
   download it and double-click to install. In Keychain Access export the certificate with its private key
   as a `.p12` with a strong password.
2. **Notarization API key.** At appstoreconnect.apple.com, Users and Access, Integrations, App Store Connect
   API, create a key with the **Developer** role. Download the `.p8` once (Apple will not show it again) and
   note the key id and the issuer id.
3. **GitHub secrets.** Create a `release` environment in the repo settings (add yourself as a required
   reviewer so a run needs approval), then set these as environment secrets with `gh secret set --env release`:

   | Secret | Value |
   | --- | --- |
   | `MACOS_CERT_P12` | the `.p12`, base64 encoded (`base64 -i cert.p12 \| pbcopy`) |
   | `MACOS_CERT_PASSWORD` | the `.p12` password |
   | `NOTARY_KEY` | the contents of the `.p8` |
   | `NOTARY_KEY_ID` | the API key id |
   | `NOTARY_ISSUER` | the API issuer id |
   | `FIGLOCK_BUILD_GOOGLE_CLIENT_SECRET` | the Google client secret for release builds (ADR-0021) |

   Keep the `.p12` and `.p8` in your password manager, then delete the loose copies.

## Running it

Trigger the workflow from the Actions tab (`Release (macOS)`, Run workflow) to check signing by hand. A `v*` tag runs `release.yml`, which calls it. A missing
secret fails the first step with the name of the secret.

## The desktop app and its dmg

`scripts/sign-macos-app.sh <FigLock.app> <out-dir>` does, in this order:

1. Signs the CLI inside the app (`Contents/MacOS/figlock`), then the app itself, both with the hardened runtime.
   Nested code is always signed before the bundle that contains it. Neither needs entitlements.
2. Notarizes the app (as a zip) and staples the ticket to it, so it opens offline once dragged out of the dmg.
3. Builds the dmg with the app and an `Applications` shortcut, signs it, notarizes it and staples it.

`scripts/notarize-macos.sh` does the notarization for both scripts, and prints Apple's log when it is rejected.
`scripts/verify-macos-app.sh <dmg>` is the check the workflow runs: the dmg, the app and the CLI are signed with a
Developer ID and a secure timestamp, have the hardened runtime and a stapled ticket, pass `spctl`, are built for the
same architectures, and the CLI inside runs.

No new secrets: the app uses the same certificate and notarization key, and the build secret
`FIGLOCK_BUILD_GOOGLE_CLIENT_SECRET` so Google Drive sign-in works in the release build.

## Signing locally

```sh
export SIGN_IDENTITY="Developer ID Application: <name> (<team id>)"
export NOTARY_KEY_PATH=/path/to/AuthKey.p8 NOTARY_KEY_ID=<id> NOTARY_ISSUER=<issuer>
cargo build --release -p figlock-cli
scripts/sign-macos.sh target/release/figlock

# the desktop app and its dmg
scripts/build-desktop.sh universal-apple-darwin
scripts/sign-macos-app.sh target/universal-apple-darwin/release/bundle/macos/FigLock.app dist
scripts/verify-macos-app.sh dist/FigLock-*-universal.dmg
```

## Checking the result on a clean Mac

1. Download `figlock.zip` from the workflow run (this sets the quarantine flag), unzip it.
2. `codesign -dvv ./figlock` shows `flags=0x10000(runtime)`, the Developer ID authority and a timestamp.
3. Run `./figlock --version`. There must be no Gatekeeper prompt. `spctl --assess` is not a reliable check for
   a bare command-line binary, so rely on running the quarantined download.

For the dmg: download `FigLock-<version>-universal.dmg` from the workflow run, open it, drag FigLock to
Applications and open it. There must be no Gatekeeper warning, also with the network off (the tickets are
stapled). In Settings, "Install command line tool" links the CLI inside the app.

## If notarization is rejected

The script prints the notary log. Common causes: missing secure timestamp, hardened runtime not enabled, or
an unsigned nested binary.
