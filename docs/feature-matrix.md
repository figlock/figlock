# FigLock feature matrix

Every feature of the three shipped surfaces, ordered from the **smallest** (one button, one flag) to the
**biggest** (whole subsystems), with a tick for each surface.

- **Mac** = the desktop app (`apps/desktop`, Tauri 2 + Svelte 5)
- **iPhone** = the iOS app and its AutoFill extension (`apps/ios`, SwiftUI, iOS 18+, beta)
- **CLI** = the `figlock` binary (`crates/figlock-cli`, macOS and Linux)

Legend: ✅ available | ❌ not available | 🔸 partly (see the note in the row)

Built from the code on branch `FIG-256-ios-tiles-appearance-vault-screen` (CLI help output, Svelte screens, SwiftUI
screens, Tauri command list). Planned work (Android, browser extension, web, Git and S3 storage, relay) is listed
at the end and is not ticked anywhere.

---

## 1. Tiny conveniences (one control, one flag)

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 1.1 | Machine-readable output | ❌ | ❌ | ✅ | Global `--json` on every command |
| 1.2 | Version flag | ❌ | ❌ | ✅ | `figlock -V` (Mac and iPhone show the version in Settings > Updates / About) |
| 1.3 | Built-in help with examples | ❌ | ❌ | ✅ | `figlock --help` and per-command help |
| 1.4 | Show or hide one secret value | ✅ | ✅ | 🔸 | CLI: `get --reveal` shows all secrets of an item at once |
| 1.5 | Secret re-masks itself after a delay | 🔸 | ✅ | ❌ | iPhone masks after 10 s. Mac shows a countdown on the copied value |
| 1.6 | Copy a field to the clipboard | ✅ | ✅ | ✅ | |
| 1.7 | Clipboard cleared automatically | ✅ | ✅ | ✅ | 30 s. CLI: `copy --clear-after <secs>` (0 keeps it) |
| 1.8 | Copy only the pasteboard item locally (no Handoff sync), expiring | ❌ | ✅ | ❌ | iPhone uses a local-only, expiring pasteboard item |
| 1.9 | Copy a non-secret field (username, public key) | ✅ | ✅ | ✅ | |
| 1.10 | Pick which field to copy or print | 🔸 | ✅ | ✅ | CLI `-f/--field`. Mac: per-row copy buttons |
| 1.11 | Back / forward navigation | ✅ | ✅ | ❌ | Mac: Cmd+[ and Cmd+] |
| 1.12 | Keyboard shortcuts | ✅ | ❌ | ❌ | Mac: Cmd+F search, Cmd+N new item, Cmd+L lock, Cmd+[ / Cmd+] |
| 1.13 | Toast confirmations (Saved, Deleted, Restored, Copied) | ✅ | ✅ | ❌ | |
| 1.14 | "Discard your changes?" guard on unsaved edits | ✅ | ✅ | ❌ | |
| 1.15 | Clear-search button | ✅ | ✅ | ❌ | |
| 1.16 | Password visibility toggle on password fields | ✅ | ✅ | ❌ | |
| 1.17 | Light / dark theme following the system | ✅ | ✅ | ❌ | |
| 1.18 | Appearance setting (System / Light / Dark) | ❌ | ✅ | ❌ | iPhone Settings > Appearance |
| 1.19 | Design tokens shared from `design/tokens.css` | ✅ | ✅ | ❌ | Same colors, type, spacing on both apps |
| 1.20 | Dynamic Type, 44 pt touch targets, Reduce Motion | ❌ | ✅ | ❌ | |
| 1.21 | Accessibility labels and screen-reader support | ✅ | ✅ | ❌ | Mac has an automated a11y E2E test |
| 1.22 | About screen (version, licenses) | ❌ | ✅ | ❌ | |
| 1.23 | Short unique-id-prefix addressing of an item | ❌ | ❌ | ✅ | `<ITEM>` is an id, id prefix or a search that finds one item |

## 2. Generating and judging passwords

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 2.1 | Random password generator | ✅ | ✅ | ✅ | `figlock generate` |
| 2.2 | Choose length | ✅ | ✅ | ✅ | CLI default 20 |
| 2.3 | Toggle symbols | ✅ | ✅ | ✅ | CLI `--no-symbols` |
| 2.4 | Toggle upper, lower, digits individually | ✅ | ✅ | ❌ | CLI only has the symbols switch |
| 2.5 | Passphrase mode (words, separator, capitalize) | ✅ | ✅ | ❌ | |
| 2.6 | Generate again / regenerate | ✅ | ✅ | ❌ | |
| 2.7 | "Use this password" straight into the item being edited | 🔸 | ✅ | ❌ | iPhone: "Use this password / passphrase". Mac: generate button in the editor |
| 2.8 | Generator works without a vault (on the lock screen) | ✅ | ❌ | ✅ | |
| 2.9 | Live password strength meter | ✅ | ✅ | ❌ | Guessability in bits |
| 2.10 | Master-password rule check at setup | ✅ | ✅ | ✅ | |

## 3. Item types and fields

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 3.1 | Login item (username, password, websites, notes) | ✅ | ✅ | ✅ | |
| 3.2 | One-time-code (TOTP) setup key stored on a login | ✅ | ❌ | ✅ | Stored as a secret. No live 6-digit code is generated anywhere yet |
| 3.3 | API token item (provider, token, env var, scopes, expiry) | ✅ | ✅ | ✅ | |
| 3.4 | Allowed hosts list on a token (the broker's allow-list) | ✅ | ✅ | ✅ | |
| 3.5 | Secure note (up to 256 KB) | ✅ | ✅ | ✅ | |
| 3.6 | Env bundle (`NAME=value` pairs) | ✅ | ✅ | ✅ | |
| 3.7 | Paste a `.env` file to fill an env bundle | ✅ | ❌ | ❌ | |
| 3.8 | SSH key item (private, public, passphrase) | ✅ | ✅ | ✅ | |
| 3.9 | Document item with up to 8 encrypted files (768 KB each) | ✅ | ✅ | ✅ | |
| 3.10 | Attach files from a picker | ✅ | ✅ | ✅ | Mac file dialog, iPhone Files app, CLI `--file` |
| 3.11 | Save an attached file back out | ✅ | ✅ | ✅ | CLI `get --file <name> --out <path\|->`, `--force` |
| 3.12 | Expiry date on tokens and documents | ✅ | ✅ | ✅ | |
| 3.13 | Several websites per login | ✅ | ✅ | ✅ | |
| 3.14 | Free-form notes on every type | ✅ | ✅ | ✅ | |
| 3.15 | Custom fields (plain or secret) on every type | ✅ | ✅ | ✅ | |
| 3.16 | Tags on items | ✅ | ✅ | ✅ | Tag chips in both apps |
| 3.17 | Size limits enforced (field caps, 8 files, 768 KB) | ✅ | ✅ | ✅ | |
| 3.18 | Stored secret never echoed into the edit form | ✅ | ✅ | ✅ | Shows "Unchanged" until you type a new value |

## 4. Finding and organising items

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 4.1 | Fuzzy search over titles, usernames, URLs, tags, file names | ✅ | ✅ | ✅ | Secrets are never searched |
| 4.2 | Empty search lists the most recent items | ✅ | ✅ | ✅ | |
| 4.3 | Filter by item type | ✅ | 🔸 | ✅ | iPhone filters: All, Logins, Notes, Other. Mac: one entry per type |
| 4.4 | Limit the number of results | ❌ | ❌ | ✅ | `find --limit` (default 20) |
| 4.5 | Subtitle under each item (username or type) | ✅ | ✅ | ✅ | |
| 4.6 | Pull-to-refresh sync | ❌ | ✅ | ❌ | |
| 4.7 | Swipe action to copy a secret from the list | ❌ | ✅ | ❌ | |
| 4.8 | Context menu / item actions menu (Edit, Delete) | ✅ | ✅ | ❌ | |
| 4.9 | "Not synced yet" marker on locally changed items | ❌ | ✅ | ❌ | |
| 4.10 | Search stays fast at 10,000 items | ✅ | ✅ | ✅ | Measured under 100 ms in a test |

## 5. Editing, deleting, restoring

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 5.1 | Add an item | ✅ | ✅ | ✅ | |
| 5.2 | Edit an item | ✅ | ✅ | ✅ | |
| 5.3 | Remove a single field | ✅ | ✅ | ✅ | CLI `--unset` |
| 5.4 | Delete to trash | ✅ | ✅ | ✅ | |
| 5.5 | Recently deleted list | ✅ | ✅ | 🔸 | CLI: `find` does not list the trash, but `export --include-deleted` does |
| 5.6 | Restore one deleted item | ✅ | ✅ | ✅ | |
| 5.7 | Recover all deleted items at once | ❌ | ✅ | ❌ | |
| 5.8 | Age shown for deleted items | ❌ | ✅ | ❌ | |
| 5.9 | Remove an attached file | ✅ | ✅ | ✅ | |
| 5.10 | Atomic save (rolled back if the write fails) | ✅ | ✅ | ✅ | |

## 6. Import and export

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 6.1 | Export all items as JSON (secrets left out) | ❌ | ❌ | ✅ | |
| 6.2 | Export with secret values | ❌ | ❌ | ✅ | `--include-secrets` |
| 6.3 | Export includes trash | ❌ | ❌ | ✅ | `--include-deleted` |
| 6.4 | Import from Chrome (CSV) | ❌ | ❌ | ✅ | |
| 6.5 | Import from Apple Passwords (CSV) | ❌ | ❌ | ✅ | |
| 6.6 | Import from Bitwarden (CSV) | ❌ | ❌ | ✅ | |
| 6.7 | Import from 1Password (CSV) | ❌ | ❌ | ✅ | |
| 6.8 | Import a plain-text notes dump | ❌ | ❌ | ✅ | Split at `---` or `===` lines |
| 6.9 | Import dry run (preview, no password asked, nothing written) | ❌ | ❌ | ✅ | |
| 6.10 | Pipe item data via stdin (`--secret` values, `-d @-`) | ❌ | ❌ | ✅ | |

## 7. Locking and unlocking

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 7.1 | Manual lock | ✅ | ✅ | ✅ | `figlock lock` |
| 7.2 | Unlock with master password + Secret Key | ✅ | ✅ | ✅ | |
| 7.3 | Quick unlock with Touch ID | ✅ | ❌ | ✅ | Mac and CLI via the Secure Enclave (or keychain fallback) |
| 7.4 | Quick unlock with Face ID | ❌ | ✅ | ❌ | |
| 7.5 | Device password fallback for quick unlock | ✅ | ✅ | ✅ | |
| 7.6 | Apple Watch approval for quick unlock | ✅ | ❌ | ✅ | Allowed by the Secure Enclave policy on Mac |
| 7.7 | Quick unlock key bound to the biometric set (ADR-0046) | ✅ | ✅ | ✅ | New fingerprint or face invalidates it |
| 7.8 | Turn quick unlock on / off / check status | ✅ | ✅ | ✅ | CLI `quick-unlock enable\|disable\|status` |
| 7.9 | Quick unlock limits and password-guess throttling | ✅ | ✅ | ✅ | |
| 7.10 | Auto-lock timer | ✅ | ✅ | ❌ | Mac: 1, 2, 5, 10, 15, 30, 60 min. iPhone: immediately, 1, 5, 15 min, 1 hour (default 5 min, no "never") |
| 7.11 | Lock when the Mac sleeps or the screen locks | ✅ | ❌ | ❌ | |
| 7.12 | Lock when the app goes to the background | ❌ | ✅ | ❌ | Honours the auto-lock time |
| 7.13 | Secret Key kept on this device (keychain, this-device-only) | ✅ | ✅ | ✅ | |
| 7.14 | Choose to stop keeping the Secret Key (ADR-0038) | ✅ | ✅ | ✅ | CLI `secret-key forget` / `keep` |
| 7.15 | Show the Secret Key after Touch ID / Face ID | ✅ | ✅ | ✅ | Mac and iPhone mask it again after a delay |
| 7.16 | Screenshot, screen-recording and app-switcher blanking of secret screens | ❌ | ✅ | ❌ | |
| 7.17 | Memory hygiene: no vault key or item values written to disk | ✅ | ✅ | ✅ | |

## 8. Credentials, recovery and Emergency Kit

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 8.1 | Change master password | ✅ | ✅ | ✅ | |
| 8.2 | Rotate the Secret Key | ✅ | ✅ | ✅ | Old key stops working everywhere |
| 8.3 | Emergency Kit PDF at vault creation | ✅ | ✅ | ✅ | With a "check you saved it" step |
| 8.4 | Re-save the Emergency Kit later | ✅ | ✅ | ✅ | CLI `secret-key kit` |
| 8.5 | Copy the Secret Key to the clipboard | ✅ | ✅ | ❌ | |
| 8.6 | Recovery Key: turn on | ✅ | ✅ | ❌ | Recovery Key PDF page, checked by typing the last two groups |
| 8.7 | Recovery Key: replace | ✅ | ✅ | ❌ | |
| 8.8 | Recovery Key: turn off | ✅ | ✅ | ❌ | |
| 8.9 | "I forgot my password" flow (reset with Recovery Key) | ✅ | ✅ | ❌ | Needs the vault folder to be present |
| 8.10 | Recovery Key lost: remove the vault from this device | ✅ | ✅ | ❌ | Never touches the copies in sync places |
| 8.11 | Credentials re-asked for sensitive changes | ✅ | ✅ | ✅ | |
| 8.12 | Recovery of a vault that exists only in Drive | 🔸 | ❌ | ❌ | Mac can restore from Drive/iCloud. iPhone notes this as not done yet |

## 9. Vault creation, opening and storage

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 9.1 | Create a vault | ✅ | ✅ | ✅ | `figlock init` |
| 9.2 | Local-only vault (no cloud) | ✅ | ✅ | ✅ | |
| 9.3 | Choose the vault folder | ✅ | ❌ | ✅ | iPhone keeps the vault in its App Group container |
| 9.4 | Name this device | ❌ | 🔸 | ✅ | CLI `--device-name`. iPhone: local nickname only |
| 9.5 | Open an existing vault with password + Secret Key | ✅ | ✅ | ✅ | `figlock open` |
| 9.6 | Open a vault found in Google Drive | ✅ | ✅ | ✅ | |
| 9.7 | Open a vault found in iCloud Drive | ✅ | ✅ | ✅ | iPhone picks the iCloud folder in Files |
| 9.8 | Open a vault from any synced folder (Dropbox, Syncthing) | ✅ | ✅ | ✅ | CLI `--sync-folder`. iPhone via Files picker |
| 9.9 | Wait for a synced folder to finish delivering the vault | ✅ | ✅ | ✅ | CLI `--wait-for-sync` |
| 9.10 | Missing-folder recovery screen (never recreates an empty vault) | ✅ | ✅ | ❌ | |
| 9.11 | "Can't unlock" explanation screen | ✅ | ✅ | ❌ | |
| 9.12 | Remove the vault from this device | ✅ | ✅ | ❌ | |
| 9.13 | Damaged / unreadable vault detection | ✅ | ✅ | ✅ | |
| 9.14 | Encrypted at rest, ciphertext-only storage | ✅ | ✅ | ✅ | Titles, URLs and usernames are encrypted too |
| 9.15 | Vault excluded from device backups, file protection class | ❌ | ✅ | ❌ | |

## 10. Sync

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 10.1 | Sync now (manual) | ✅ | ✅ | ✅ | |
| 10.2 | Sync status line (last synced, syncing, paused, offline) | ✅ | ✅ | ✅ | |
| 10.3 | Sync to Google Drive (hidden app folder only) | ✅ | ✅ | ✅ | `drive.appdata` scope, nothing else |
| 10.4 | Sync to iCloud Drive | ✅ | ✅ | ✅ | Mac and CLI: automatic `FigLock` folder. iPhone: pick a folder |
| 10.5 | Sync to any other synced folder | 🔸 | ✅ | ✅ | Mac lists and manages them, but adds them from CLI or setup |
| 10.6 | Several sync places at once, each independent | ✅ | ✅ | ✅ | One failing never blocks another (ADR-0043) |
| 10.7 | Turn sync off for one place or all | ✅ | ✅ | ✅ | Data and stored copies are left alone |
| 10.8 | Per-place status and error message | ✅ | ✅ | ✅ | CLI `sync status` |
| 10.9 | Sign in to Google (browser, PKCE) | ✅ | ✅ | ✅ | |
| 10.10 | Sign out of Google (revokes access) | ✅ | ✅ | ✅ | |
| 10.11 | Switch to a different Google account | ✅ | 🔸 | 🔸 | iPhone and CLI: sign out then in again |
| 10.12 | Show the signed-in Google account email | ❌ | ✅ | ❌ | Mac cannot see it with the app-folder scope |
| 10.13 | Re-upload after the remote copy was lost | ✅ | ✅ | ✅ | CLI `sync --reupload` |
| 10.14 | Delete the vault's copy from Google Drive (typed phrase) | ✅ | ✅ | ✅ | CLI `sync delete-remote` |
| 10.15 | Refuse to sync into a Drive holding a different vault | ✅ | ✅ | ✅ | |
| 10.16 | Offline handling with back-off | ✅ | ✅ | ✅ | |
| 10.17 | Detect that another device changed credentials, prompt re-unlock | ✅ | ✅ | ✅ | |
| 10.18 | Fork warning (two devices changed settings at once) | ✅ | ❌ | ❌ | |
| 10.19 | Background sync | ❌ | ✅ | ❌ | iOS BGAppRefresh task |
| 10.20 | Foreground sync loop | ✅ | ✅ | ❌ | |
| 10.21 | Safe-folder checks (folder may not contain or sit inside the local vault) | ✅ | ✅ | ✅ | |
| 10.22 | Evicted iCloud file handling ("Optimize Mac Storage") | ✅ | ✅ | ✅ | Requests a download, never treats it as missing |
| 10.23 | Operation-log merge that converges across devices | ✅ | ✅ | ✅ | Property-tested in `figlock-core` |

## 11. Devices and pairing

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 11.1 | List the devices that can open the vault | ✅ | ✅ | ✅ | |
| 11.2 | Mark "this device" | ✅ | ✅ | ✅ | |
| 11.3 | Device fingerprint shown | ✅ | ✅ | ✅ | |
| 11.4 | Join an existing vault with an invite code | ✅ | ✅ | ✅ | `figlock join <code>` |
| 11.5 | Verify a short code or fingerprint during pairing | ✅ | ✅ | ✅ | |
| 11.6 | Approve a new device (show invite code) | ✅ | ❌ | ✅ | iPhone can join but not approve others |
| 11.7 | Revoke a device | ✅ | ✅ | ✅ | Needs master password and Secret Key on iPhone |
| 11.8 | Rename a device | ❌ | 🔸 | ❌ | iPhone nickname is shown on that iPhone only |
| 11.9 | "This device was removed" screen / banner | ✅ | ❌ | ❌ | |
| 11.10 | Pairing timeout and cancel | ✅ | ✅ | ✅ | CLI `--timeout` (default 600 s) |
| 11.11 | Revoked-device name kept in vault.json (ADR-0036) | ✅ | ✅ | ✅ | |

## 12. Password health

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 12.1 | Weak password detection | ✅ | ❌ | ❌ | |
| 12.2 | Reused password detection | ✅ | ❌ | ❌ | |
| 12.3 | Old password detection | ✅ | ❌ | ❌ | |
| 12.4 | Known-breach check (opt-in, online) | ✅ | ❌ | ❌ | |
| 12.5 | "Items that need attention" list linking to each item | ✅ | ❌ | ❌ | |

## 13. Agent access (the broker)

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 13.1 | Install the Claude skill that teaches agents the broker | ❌ | ❌ | ✅ | `figlock skill install` / `show` |
| 13.2 | List active grants (never their tokens) | ❌ | ❌ | ✅ | `broker grants` |
| 13.3 | Revoke a grant now | ❌ | ❌ | ✅ | `broker revoke` |
| 13.4 | Broker status (running, grants, audit position) | ✅ | ❌ | ✅ | |
| 13.5 | Lock the broker (forget vault and all grants) | ✅ | ❌ | ✅ | |
| 13.6 | Run the broker daemon | ✅ | ❌ | ✅ | Mac app hosts it itself (ADR-0040). CLI runs `broker start` in the foreground |
| 13.7 | Take over the broker socket from a CLI-run broker | ✅ | ❌ | ❌ | |
| 13.8 | Approval prompt with Touch ID | ✅ | ❌ | ✅ | Mac shows a card, CLI prompts in the terminal |
| 13.9 | Approval card shows program, path, parent chain, reason | ✅ | ❌ | 🔸 | Mac card also marks "checked by your Mac" and loud warnings |
| 13.10 | Unanswered request counts as "no" after a timeout | ✅ | ❌ | ✅ | |
| 13.11 | Grant lifetimes: once, 15 min, 1 day, until lock | ❌ | ❌ | ✅ | `--ttl` |
| 13.12 | Reason text shown to the approver | ❌ | ❌ | ✅ | `--reason` (unchecked text, labelled so) |
| 13.13 | One prompt per request, grant reuse (ADR-0039) | ✅ | ❌ | ✅ | |
| 13.14 | `figlock run`: inject secrets as environment variables | ❌ | ❌ | ✅ | Output scrubbed of secret values |
| 13.15 | Run with a single token item | ❌ | ❌ | ✅ | `--item` |
| 13.16 | Run with a whole env bundle | ❌ | ❌ | ✅ | `--bundle` |
| 13.17 | Map variables to item fields | ❌ | ❌ | ✅ | `--env NAME=field` |
| 13.18 | `figlock http`: request made by the broker, secret added by it | ❌ | ❌ | ✅ | Agent never sees the token |
| 13.19 | Host allow-list enforced per token | ✅ | ❌ | ✅ | |
| 13.20 | https only (plain http only to this machine) | ❌ | ❌ | ✅ | |
| 13.21 | Custom method, headers, body, `@-` stdin | ❌ | ❌ | ✅ | `-X`, `-H`, `-d` |
| 13.22 | Custom auth header and scheme, secret field choice | ❌ | ❌ | ✅ | `--auth-header`, `--auth-scheme`, `--field` |
| 13.23 | Include response headers, fail on HTTP error (exit 22) | ❌ | ❌ | ✅ | `-i`, `--fail` |
| 13.24 | Signed, encrypted, hash-chained audit log | ✅ | ❌ | ✅ | |
| 13.25 | Verify and print the audit log | ❌ | ❌ | ✅ | `figlock audit`, `--tail` |
| 13.26 | Fail closed if the audit record cannot be written | ✅ | ❌ | ✅ | |
| 13.27 | Verified program identity (code-signing check of the caller) | ✅ | ❌ | ✅ | `figlock-procinfo` |
| 13.28 | Reveal to an agent (opt-in, approval every time) | ❌ | ❌ | 🔸 | Specified in the broker spec. Not a user-facing command |

## 14. Command-line tool management

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 14.1 | Install the CLI from inside the Mac app (opt-in) | ✅ | ❌ | ❌ | Settings > Command line tool |
| 14.2 | Detect "already installed" / "needs repair" | ✅ | ❌ | ❌ | |
| 14.3 | Remove the CLI | ✅ | ❌ | ❌ | |
| 14.4 | Install via Homebrew or `curl figlock.com/install.sh` | ❌ | ❌ | ✅ | Checked against `SHA256SUMS` |
| 14.5 | Linux builds (x86_64, aarch64) | ❌ | ❌ | ✅ | Published, lightly tested |

## 15. Updates and distribution

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 15.1 | In-app update check and one-click install | ✅ | ❌ | ❌ | Settings > Updates, with release notes |
| 15.2 | Restart to finish an update | ✅ | ❌ | ❌ | |
| 15.3 | Signed and notarized | ✅ | ❌ | ✅ | macOS binaries |
| 15.4 | Universal binary (Apple silicon + Intel) | ✅ | ❌ | ❌ | |
| 15.5 | App Store / TestFlight distribution | ❌ | 🔸 | ❌ | iPhone: beta, release coming |

## 16. OS integration

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 16.1 | iOS AutoFill credential provider extension | ❌ | ✅ | ❌ | Read-only, Face ID only (ADR-0041) |
| 16.2 | AutoFill matches logins by website | ❌ | ✅ | ❌ | |
| 16.3 | AutoFill falls back to all logins, with search, when none match | ❌ | ✅ | ❌ | "No saved login for <site>" |
| 16.4 | AutoFill identities kept in the system store | ❌ | ✅ | ❌ | |
| 16.5 | Open a file in Files / save to Files | ✅ | ✅ | ❌ | |
| 16.6 | Share sheet for the Emergency Kit | ❌ | ✅ | ❌ | |
| 16.7 | Tauri hardened CSP, no remote content | ✅ | ❌ | ❌ | |
| 16.8 | Window only receives what a screen shows (no vault key in the webview) | ✅ | ❌ | ❌ | |

## 17. Onboarding and setup flows (the big screens)

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 17.1 | Welcome screen with create / open / join / forgot | ✅ | ✅ | ❌ | |
| 17.2 | Create a vault wizard (storage, password, Secret Key, kit check) | ✅ | ✅ | ✅ | CLI: `init` |
| 17.3 | Choose where the vault is kept (Drive, iCloud, folder) | ✅ | ✅ | ✅ | |
| 17.4 | Offer a Recovery Key during setup | ✅ | ✅ | ❌ | |
| 17.5 | Offer quick unlock during setup | ✅ | ✅ | ✅ | CLI: `--quick-unlock` / `--no-quick-unlock` |
| 17.6 | Join with an invite code, wait for approval | ✅ | ✅ | ✅ | |
| 17.7 | Debug demo vault for testing | ❌ | ✅ | ❌ | Debug builds only |

## 18. Quality and test coverage

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 18.1 | Automated E2E tests | ✅ | ✅ | ✅ | Playwright (Mac), XCUITest (iPhone), integration tests (CLI) |
| 18.2 | Unit tests on logic | ✅ | ✅ | ✅ | |
| 18.3 | Crypto and merge test vectors (`docs/spec/`) | ✅ | ✅ | ✅ | Shared Rust core |
| 18.4 | Fuzzed importers | ❌ | ❌ | ✅ | |
| 18.5 | App Store screenshot automation | ❌ | ✅ | ❌ | |

## 19. Shared cryptographic foundation (the biggest piece)

Every surface runs the same Rust core (`figlock-core`, `figlock-store`, `figlock-sync`, `figlock-session`).

| # | Feature | Mac | iPhone | CLI | Notes |
| --- | --- | :-: | :-: | :-: | --- |
| 19.1 | Argon2id password hashing | ✅ | ✅ | ✅ | |
| 19.2 | HKDF key derivation | ✅ | ✅ | ✅ | |
| 19.3 | XChaCha20-Poly1305 encryption | ✅ | ✅ | ✅ | |
| 19.4 | X25519 key agreement (device pairing) | ✅ | ✅ | ✅ | |
| 19.5 | Ed25519 signatures (records, audit log) | ✅ | ✅ | ✅ | |
| 19.6 | Two-secret model: master password + Secret Key | ✅ | ✅ | ✅ | |
| 19.7 | Secure Enclave bound quick-unlock key | ✅ | ✅ | ✅ | |
| 19.8 | Append-only operation log, one file per commit | ✅ | ✅ | ✅ | |
| 19.9 | Hybrid logical clock for merge ordering | ✅ | ✅ | ✅ | |
| 19.10 | Storage adapters: local folder, Google Drive | ✅ | ✅ | ✅ | |
| 19.11 | No account, no server, no database of ours | ✅ | ✅ | ✅ | |
| 19.12 | Open source (MIT or Apache-2.0) | ✅ | ✅ | ✅ | |

---

## Not built yet (nothing ticked)

| Feature | Status |
| --- | --- |
| Android app | Planned, its own phase (ADR-0042) |
| Browser extension (WXT) | Planned |
| Web app (wasm) | Planned |
| Git and S3 storage adapters | Planned, "later" |
| Optional relay for remote approvals (`relay.figlock.com`) | Planned |
| Live TOTP code generation | Not built. Only the setup key is stored |
| Mac AutoFill / browser autofill | Not built (needs the extension) |
| Import and export screens in the Mac and iPhone apps | Not built (CLI only) |
| Password health on iPhone and CLI | Not built (Mac only) |
| Agent reveal as a user command | Spec only |

## Counts (rows with a full tick)

Generated by hand from the tables above, so re-count if rows change.

| Surface | Unique to it | Shared with at least one other |
| --- | --- | --- |
| CLI | Import, export, `run`, `http`, `audit`, grants, skill install, JSON mode | Everything in sections 1 to 11 that it ticks |
| Mac | Password health, CLI installer, updates, fork warning, `.env` paste, sleep-lock | Vault CRUD, sync, pairing, recovery |
| iPhone | AutoFill extension, Face ID, background sync, secret-screen blanking, Appearance | Vault CRUD, sync, pairing, recovery |
