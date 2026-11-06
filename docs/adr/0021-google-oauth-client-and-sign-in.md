# ADR-0021: Google sign-in uses a compiled-in installed-app client, loopback and PKCE

- Status: Accepted
- Date: 2026-11-06
- Decision: D21 in `docs/decisions.md`

## Context

The Drive adapter (ADR-0010) needs an OAuth sign-in on every device. Google's "Desktop app" client type is
meant for this: loopback redirect plus PKCE, with a client secret that Google documents as non-confidential
because an installed app cannot keep one. The hidden appDataFolder belongs to the Google Cloud project
(`figlock-prod`), so every FigLock install has to use that project's client for the data to be shared across
devices. Tools such as rclone and gcloud ship their client the same way.

## Decision

- The client ID and secret are compiled into the binary and can be overridden with
  `FIGLOCK_GOOGLE_CLIENT_ID` and `FIGLOCK_GOOGLE_CLIENT_SECRET` (forks, heavy users, quota spreading).
- Sign-in is authorization code with a loopback redirect on `127.0.0.1` and PKCE S256. The listener only
  accepts a request carrying our random `state`. Only the `drive.appdata` scope is requested, and a grant
  of anything else is refused.
- The refresh token is stored owner-only (`drive-token.json`, mode 0600) beside the device keys, moving to
  the platform keystore with them. It is never logged or printed. `figlock drive logout` revokes it at Google
  (best effort, the secret is not needed) and always deletes the local file, even if it is damaged or its
  permissions are loose.

## Consequences

- Works out of the box with no Google Cloud setup by the user.
- Anyone can extract the secret from the binary. They can impersonate the FigLock consent screen or burn the
  project's API quota; they cannot read anyone's data. PKCE makes a stolen code useless, vault contents are
  ciphertext, and devices keep a full local copy (ADR-0010). The secret can be rotated in the Cloud console and
  a new release shipped.
- While the consent screen is in Testing status, Google expires refresh tokens after 7 days and caps test
  users. Publishing the app (FIG-66 then FIG-46) is required before real use.

## Alternatives considered

- Inject the secret at build time: it still ships in the binary, so no security gain, and source builds,
  Homebrew-from-source and reproducible builds (FIG-57) break.
- Every user creates their own Google Cloud client: breaks the three-minute onboarding goal.
