# ADR-0031: Keychain items are readable by any app of our developer team

- Status: Accepted
- Date: 2026-11-14
- Decision: D31 in `docs/decisions.md`

## Context

FigLock keeps three kinds of small secret in the macOS login keychain: the device keys
(`com.figlock.cli.device`), the Secret Key (`com.figlock.cli.secret-key`, ADR-0027) and, when Secure Enclave is
not used, the quick unlock key (`com.figlock.cli`). The command line tool and the desktop app are two different
apps that must open the same vault on the same Mac with one device identity (ADR-0025, ADR-0027).

A new keychain item gets an access list that trusts only the app that made it, matched by that app's code
identifier. On a signed release build this was reproduced (FIG-106): a vault made with the CLI (identifier
`figlock`) was opened by the desktop app (identifier `com.figlock.desktop`) and macOS asked for the login
keychain password ("FigLock wants to use your confidential information stored in `com.figlock.cli.device`"),
before the Touch ID unlock could even run. Denying it failed the unlock. The same would happen the other way
round for a vault the desktop app made.

The item also has a partition list, which macOS fills with the creating app's team (`teamid:<team>`) and checks
in addition to the per-app list. It was already correct on the reproduced item.

## Decision

- **Items are created with a read permission that trusts any application.** The partition list, which names
  our developer team, then decides: an app signed by our team reads the item with no prompt, and any other app
  (a different team, an unsigned binary, Apple's own `security` tool) is prompted for the keychain password
  exactly as before. This was checked on a build: `security find-generic-password -w` on such an item still
  blocks on the prompt.
- **One helper creates them** (`figlock_biometric::keychain`, the one crate that may use `unsafe`, because the
  safe `security-framework` API cannot set an access list). `figlock-session` calls it for all three kinds of
  item; reading and deleting are unchanged. An item from before this change is replaced on its next write so it
  gets the new list; if it cannot be removed, the secret is updated in place and the old list stays.
- **Unsigned and development builds have no team**, so their partition is the binary's own code hash and only
  that exact binary reads the item. They gain nothing and lose nothing.
- **Not chosen:** a shared keychain access group (needs the Data Protection keychain, a provisioning profile and
  entitlements, which a bare command-line binary and the Homebrew install cannot have); separate device
  identities for the CLI and the app (opening a vault in the app would need pairing every time); signing both
  binaries with the same code identifier (fragile, and ties two products together).

## Consequences

- No prompt when the signed desktop app opens a vault made by the signed CLI, or the other way round.
- Any process signed by our team can read the items without a prompt. Those are our own binaries.
- A signed build and an unsigned local build cannot read each other's items without the prompt; that is
  development only.
- Items already written by an earlier build keep the old list until they are next written (a full unlock
  rewrites the Secret Key; creating or joining rewrites the device keys). Before the first release this affects
  development machines only.
- No change to the crypto, the vault format or the device list.
