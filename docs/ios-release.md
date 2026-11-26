# Releasing the iPhone app to TestFlight

A version tag builds the iPhone app and uploads it to App Store Connect, where it appears in TestFlight for your
internal testers (FIG-175). `release-ios.yml` does it, next to `release.yml` (Mac app and CLI) on the same `v*` tag.
It never publishes to the App Store: a build waits in TestFlight until someone submits it for review (FIG-55).
What goes into App Store Connect (privacy answers, review notes, export compliance) is in `docs/ios-app-store.md`.

## How a build is made

- **Version.** The app follows the Cargo workspace version, like the Mac app. `scripts/ios-version.sh` checks that the
  tag is `v<workspace version>` and gives `MARKETING_VERSION`. A pre-release suffix is dropped, because App Store
  Connect only takes numbers: `v0.6.0-rc1` builds as 0.6.0.
- **Build number.** The workflow run number, which only goes up, so every upload of a version is accepted.
  Re-running a failed run keeps its number, so a retry after a half-finished upload can be refused as a duplicate:
  push a fresh tag or run the workflow again by hand instead of re-running the old job.
- **Signing.** Automatic and cloud-managed. `xcodebuild -allowProvisioningUpdates` with the App Store Connect API key
  registers the bundle ids, the App Group and the keychain group, and creates and manages the distribution
  certificate and profiles at Apple. There is no certificate or profile to store in GitHub.
- **Upload.** `xcodebuild -exportArchive` with `destination: upload` signs for distribution and sends the build to
  App Store Connect. The archive's debug symbols are kept as a workflow artifact.
- **Checks on the way.** The job fails before building if a secret is missing, and after archiving if the app is not
  the expected version and build number, is not validly signed or lacks the AutoFill extension.

## One-time setup (account holder, not an agent)

Never put the key or any id in git, issues, logs or screenshots.

1. **Developer account.** The paid Apple Developer account from FIG-44. Note the Team ID (Membership details).
2. **App Store Connect API key.** At appstoreconnect.apple.com, Users and Access, Integrations, App Store Connect API,
   create a **team key** with the **Admin** role (cloud-managed signing needs a role that can manage certificates
   and identifiers; if the first run says it lacks permission, this is the thing to check). Download the `.p8` once
   and note the key id and issuer id. This is a different key from the notarization key the Mac release uses; you
   can reuse that one only if it has the Admin role.
3. **App record.** Create the app in App Store Connect with bundle id `com.figlock.ios` (steps and answers in
   `docs/ios-app-store.md`, section 5). The first upload needs it to exist. The extension's id
   `com.figlock.ios.autofill` and the App Group `group.com.figlock.ios` are registered by the first signed archive.
4. **GitHub secrets.** In the `release` environment (the same one the Mac release uses), set with
   `gh secret set --env release`:

   | Secret | Value |
   | --- | --- |
   | `ASC_KEY` | the contents of the `.p8` |
   | `ASC_KEY_ID` | the API key id |
   | `ASC_ISSUER_ID` | the API issuer id |
   | `APPLE_TEAM_ID` | the Team ID |

   Keep the `.p8` in your password manager, then delete the loose copy.
5. **Testers.** In App Store Connect, TestFlight, add yourself as an internal tester (an App Store Connect user with
   access to the app).

## Cut a TestFlight build

Tag as in `docs/releasing.md` (the tag must match the workspace version). To try a build without cutting a release,
start `Release (iOS)` by hand from `main` in the Actions tab: it builds the workspace version and uploads it to
TestFlight only, and nothing public is released. Approve the `release` environment when the
`Release (iOS)` run asks. To retry from a tag without a new run of the Mac release, start `Release (iOS)` by hand from
the Actions tab and pick the tag. After a few minutes, App Store Connect shows the build as processing, then it
appears in TestFlight; open the TestFlight app on the iPhone and install it.

## Export compliance

The app declares `ITSAppUsesNonExemptEncryption` as `true` (`docs/ios-app-store.md`, section 1), so App Store Connect
asks the encryption questions for a build until it has a compliance code. After the first build is answered, Apple
shows a code. Store it as a repository variable named `ITS_EXPORT_COMPLIANCE_CODE` (Settings, Secrets and variables,
Actions, Variables; it is an identifier, not a secret). The workflow then writes it into `Info.plist` as
`ITSEncryptionExportComplianceCode` and later builds skip the question.

## Checking a build on the phone

1. Install it from TestFlight on your iPhone. It opens at the welcome screen (a TestFlight build does not carry the
   debug demo seed).
2. Create a throwaway vault, confirm Face ID prompts, turn on AutoFill in Settings, General, AutoFill &
   Passwords, and check the vault list and Settings.
3. TestFlight shows the version and build next to the app name. The version must be the tag's, and the build number the run number of the workflow.
4. If something is off, fix it on `main`, bump and tag again.

## Troubleshooting

| Symptom | Likely cause |
| --- | --- |
| `secret ... is not set` | A secret is missing from the `release` environment |
| `Communication with Apple failed` or `No profiles for ...` | The API key's role is too low, or the app record does not exist yet |
| `You already have a distribution certificate` or a certificate limit | Delete an unused distribution certificate in the developer account; cloud-managed ones are reused |
| Upload refused as a duplicate build | A run was re-run with the same number; start a new run |
| Build stuck on "Missing compliance" in TestFlight | Answer the encryption questions in App Store Connect, then set `ITS_EXPORT_COMPLIANCE_CODE` |
| `ITMS-91053` missing API declaration | Add the category to both `PrivacyInfo.xcprivacy` files (`docs/ios-app-store.md`, section 2) |
