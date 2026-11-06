# ADR-0021: Google sign-in uses an installed-app client, loopback and PKCE; the secret is injected at build time

- Status: Accepted (revised 2026-11-06, same day: the secret moved from source to the build)
- Date: 2026-11-06
- Decision: D21 in `docs/decisions.md`

## Context

The Drive adapter (ADR-0010) needs an OAuth sign-in on every device. Google's "Desktop app" client type is
meant for this: loopback redirect plus PKCE, with a client secret that Google documents as non-confidential
because an installed app cannot keep one. The hidden appDataFolder belongs to the Google Cloud project
(`figlock-prod`, FIG-40), so every FigLock install has to use that project's client for the data to be shared
across devices.

Two things shaped the decision:

- **GitHub push protection blocks the secret.** Committing the real client secret was rejected at push time
  (`GH013`, "Push cannot contain secrets"). Getting past it means a per-push bypass by the repo owner and a
  permanent alert on the repo, and a Google client secret in a public repo may trigger partner alerts and a
  reset by Google (not verified; a reset would break sign-in for every installed copy until a new release).
  This repo's rule is that secrets never live in it.
- **What comparable projects do** (checked 2026-11-06):
  - rclone compiles a shared client in: the client ID is plaintext and the secret is "obscured" with its own
    reversible `obscure` routine, so it is recoverable from the binary and unlikely to be caught by scanners.
    Its docs say the client is shared between all users, has a default quota of about 10 transactions per
    second, and the source now says the shared ID "is being retired and will stop working during 2026", so
    users are told to create their own.
  - Chromium injects `google_api_key`, `google_default_client_id` and `google_default_client_secret` at build
    time (`args.gn`) or from environment variables at run time. Forks and derivatives must get their own keys.
  - Thunderbird distro builds (Arch, CentOS) inject OAuth keys at build time with
    `--with-google-oauth-api-keyfile`, using the distro's own client.

## Decision

- **Client ID** (`994923112244-...apps.googleusercontent.com`) is compiled in. It is public by nature.
- **Client secret** is injected when the official binaries are built, from the `FIGLOCK_BUILD_GOOGLE_CLIENT_SECRET`
  build variable (`option_env!` in `gauth.rs`). It is never committed. At run time `FIGLOCK_GOOGLE_CLIENT_SECRET`
  (and `FIGLOCK_GOOGLE_CLIENT_ID`) override it.
- A build without the variable, such as any source build, has no built-in secret. `figlock drive login` then
  fails with a message saying to install a release build or set the run-time variables. `figlock drive logout`
  still works, because Google's revoke endpoint needs only the token.
- **We do not obscure the secret** in source the way rclone does. That exists to get past scanners and hides a
  real credential from reviewers, against the rule above.
- Sign-in is authorization code with a loopback redirect on `127.0.0.1` and PKCE S256. The listener only
  accepts a request carrying our random `state`. Only the `drive.appdata` scope is requested, and a grant of
  anything else is refused.
- The refresh token is stored owner-only (`drive-token.json`, mode 0600) beside the device keys, moving to the
  platform keystore with them (FIG-69). It is never logged or printed. `figlock drive logout` revokes it at
  Google and deletes the local copy even when the file is damaged or has loose permissions.
- There is deliberately no environment override for the OAuth or Drive endpoints: a hostile environment could
  then receive the bearer token.

## Who gets a working sign-in

| Channel | Compiled by | Has the secret |
| --- | --- | --- |
| Release binary, `figlock.com/install.sh`, our Homebrew tap (prebuilt), desktop app | Our CI release workflow | Yes |
| `cargo build` from a clone, `cargo install`, Homebrew building from source | The user | No: set `FIGLOCK_GOOGLE_CLIENT_SECRET`, or bring your own client |
| Distro packages (Debian, Nix, AUR) and homebrew-core | The packager | No: they need their own client, as Chromium and Thunderbird packagers do. Stay in our own tap so Homebrew users get the prebuilt binary |

Customers who install an official release are unaffected. Only people building from source see the difference.

## Release workflow requirements (FIG-32)

- Store the secret as a GitHub Actions secret, ideally on a protected `release` environment limited to tags or
  `main` so pull requests from forks can never read it.
- Pass it to the build as `FIGLOCK_BUILD_GOOGLE_CLIENT_SECRET`. The release job must fail before building if
  it is empty, and after building it should confirm the binary carries a Google secret (for example
  `strings figlock | grep -q 'GOCSPX-'`), so a missing or misnamed secret cannot ship a binary whose sign-in is
  silently broken.
- Rotating the secret means: create the new secret in the Cloud console, update the CI secret, ship a release.
  Old binaries keep working until the old secret is deleted in the console, so keep it until installs update.

## Consequences

- The repo contains no secret and trips no scanner. Out-of-the-box sign-in works for official builds only.
- The secret is still recoverable from any shipped binary (`strings`). Anyone who has it can impersonate the
  FigLock consent screen or burn the project's API quota; they cannot read anyone's data. PKCE makes a stolen
  code useless, vault contents are ciphertext, and every device keeps a full local copy (ADR-0010). Rotation is
  the remedy.
- Reproducible builds (FIG-57) take the secret as a documented build input, so only builds made with the same
  client can be compared bit for bit.
- A shared client has shared quota. If quota or Google verification becomes a problem (the rclone experience),
  options are requesting higher quota, publishing the app, or giving heavy users their own client via the
  run-time variables.
- While the consent screen is in Testing status, Google expires refresh tokens after 7 days and caps test
  users (listed accounts only; anyone else gets `access_denied`). Publishing the app (FIG-66, then FIG-46) is
  required before real use.

## Alternatives considered

- Commit the secret in source: blocked by push protection, leaves a permanent alert, and risks an external
  reset. Rejected.
- Obscure the secret in source like rclone: evades the scanner without removing the exposure, and hides it from
  review. Rejected.
- Every user creates their own Google Cloud client: breaks the three-minute onboarding goal.
- A relay of ours that holds the secret and exchanges codes: adds a server that touches credentials, against the
  "no backend" rule.
