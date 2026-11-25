# iPhone app: App Store and TestFlight answers (FIG-174)

What goes into App Store Connect for the FigLock iPhone app, and why. The code side is the privacy manifests
(`apps/ios/App/PrivacyInfo.xcprivacy`, `apps/ios/AutofillExtension/PrivacyInfo.xcprivacy`), `ITSAppUsesNonExemptEncryption`
in `apps/ios/App/Info.plist`, and the pages on figlock.com. The release workflow that uploads builds is FIG-175.

Never put a real vault, token, Recovery Key or personal detail in App Store Connect, review notes or screenshots.

## 1. Encryption export compliance

`ITSAppUsesNonExemptEncryption` is `true`: FigLock encrypts user data with its own library (XChaCha20-Poly1305,
Argon2id, HKDF, X25519, Ed25519 from RustCrypto), which goes beyond the encryption built into iOS.

Answers in App Store Connect (once, on the first build; it is asked again for each build until a compliance code is set):

| Question | Answer |
| --- | --- |
| Does the app use encryption? | Yes |
| Does it use encryption other than, or in addition to, what is in the OS? | Yes |
| Does it qualify for an exemption under the US export rules? | Yes: standard, publicly specified algorithms only, no proprietary or non-standard cryptography, mass-market app |
| Is it available in France? | Decide at launch. Apple may ask for documentation for France. Leaving France out of availability avoids it. |

After the first build is answered, Apple can issue an `ITSEncryptionExportComplianceCode`. Put it in `Info.plist` so
later TestFlight builds skip the question (FIG-175 adds the key to the build once the code exists).

Rules on exports change, and this is not legal advice. The account holder confirms the answers against Apple's
export compliance page and the BIS rules for mass-market encryption. Open point: whether the public source repository
needs a one-time BIS notice. We believe not for standard algorithms; confirm.

## 2. App Privacy (nutrition label)

Answer **Data Not Collected**. FigLock has no server, no accounts, no analytics and no crash reporting of its own.
The vault stays on the phone and in storage the person chose (their Google Drive app folder or a Files folder).
Sign-in to Google goes to Google, and no data reaches us.

- Tracking: No. The privacy manifests set `NSPrivacyTracking` to false and list no tracking domains.
- Required-reason APIs declared: file timestamps (`C617.1` for the app group container, `3B52.1` for a folder the
  person picked in Files). Disk space and boot time are not used by FigLock code. The Rust library links `rustix`,
  which imports `fstatfs` and `fstatvfs`. If App Store Connect reports a missing declaration for disk space on the
  first upload, add the category with reason `E174.1` and note it here.
- Privacy policy URL: https://figlock.com/privacy/
- Support URL: https://figlock.com/support/

## 3. Review notes (paste into App Review Information)

Sign-in is not required: FigLock has no accounts. Leave "Sign-in required" off and use these notes.

> FigLock is a password and secrets manager with no server and no account. Everything is created on the device.
> No demo account is needed. To review it:
>
> 1. Open the app and choose "Create a new vault".
> 2. Choose any master password (for example a long made-up phrase) and continue.
> 3. Step 2 shows a Secret Key. Step 3 offers the Emergency Kit PDF through the share sheet: Save to Files or
>    close the sheet. Step 4 asks for the last two groups of the Secret Key (eight characters) shown on step 2.
> 4. The vault opens. Use the plus button to add a login, an API token or a note. Tap an item to see it. Copy
>    clears the clipboard after 30 seconds. Showing a secret asks for Face ID or the passcode and hides it after
>    10 seconds.
> 5. Settings has auto-lock, the Emergency Kit, the Recovery Key, devices and where the vault syncs.
> 6. AutoFill: Settings, General, AutoFill & Passwords, turn on FigLock. It reads the vault on the phone only and
>    needs Face ID.
>
> Face ID is optional and only unlocks the vault on this phone (iOS does the check; FigLock never receives face
> data). Syncing to Google Drive or a Files folder is optional and off until the person sets it up. The app does
> not contact any FigLock server. Encryption uses standard algorithms (XChaCha20-Poly1305, Argon2id, X25519,
> Ed25519) and is declared in the export compliance answers.

Contact for review: use a monitored address (support@figlock.com) and a phone number the account holder provides.

### Demo vault for the reviewer

None is shipped. A prefilled vault would need a real Secret Key in the notes, and a build that creates its own is
simpler and does not risk a reviewer locking out a shared vault. The debug-only `-figlock-seed-demo` seed is not
compiled into release builds (`apps/ios/App/DebugSeed.swift`). If review asks for sample content, the account
holder can add made-up items on a throwaway vault and share only the steps, never the keys.

## 4. Screenshots

Taken from the app on an iPhone 18 Pro Max simulator (1320 x 2868, the 6.9 inch size Apple scales for smaller
phones), status bar set to 9:41, made-up items only. Light and dark are in `design/prototypes/ios/app-store/`:

| File | Screen |
| --- | --- |
| `01-welcome-*.png` | Welcome and the four ways to start |
| `02-lock-*.png` | Lock screen |
| `03-vault-*.png` | Vault list |
| `04-item-detail-light.png` | An API token with its secret masked |
| `05-settings-light.png` | Settings |

The first three are launched with `-figlock-fresh`, `-figlock-seed-demo` and `-figlock-unlock-demo`. The last two are
taken by `apps/ios/scripts/app-store-screenshots.sh`, which runs an opt-in UI test that navigates while the script
takes the picture with `simctl` (the app hides itself from in-process screenshots on purpose). The test does nothing
unless `FIGLOCK_SCREENSHOT_DIR` is set. Retake all of them when the design changes, with the same made-up data
(`mira@example.com`, `Stripe API key`) as the desktop screenshots. Not captured yet: the generator. Dark variants of 04 and 05 can be added by running the script with the
simulator in dark mode.

## 5. Checklist for the account holder (App Store Connect)

1. Register the bundle ids `com.figlock.ios` and `com.figlock.ios.autofill`, the App Group
   `group.com.figlock.ios` and the keychain group (FIG-175 finishes this with signing).
2. Create the app record: name, primary language, bundle id, SKU, category (Utilities or Productivity).
3. Fill in the App Privacy answers from section 2, the privacy policy and support URLs.
4. Answer export compliance from section 1.
5. Age rating: no objectionable content, no user-generated content shared with others, no web access beyond sign-in.
6. Upload the screenshots from section 4 and write the description, keywords and "What's new".
7. Paste the review notes from section 3 and set the contact details.
8. For TestFlight, add the internal testers and fill "Beta App Description" and the feedback address.
