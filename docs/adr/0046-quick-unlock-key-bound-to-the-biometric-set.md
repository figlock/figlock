# ADR-0046: The iPhone quick unlock key is bound to the current biometric set

- Status: Accepted
- Date: 2026-11-26
- Decision: D46 in `docs/decisions.md`
- Amends: ADR-0041 decision 6 (keys, "Biometric set changes"), `crypto.md` 8 (quick unlock, iOS); uses the option ADR-0025 left open ("the stricter current-biometric-set flag is a possible later opt-in")
- Task: FIG-247

## Context

ADR-0041 ended quick unlock on a phone when the system reports a different enrolled biometric set than when it was
turned on, because someone who knows the passcode can add a face. The code did it by saving `evaluatedPolicyDomainState`
when Face ID was turned on and comparing it with the value read at each unlock.

On the first TestFlight builds that broke AutoFill. The AutoFill extension read a state of the same size as the
app's saved one but a different value ("saved 32 bytes, now 32 bytes, different"): iOS reports that state per process.
Every AutoFill attempt looked like "Face ID changed", ended Face ID, deleted the key, and refused to show the Face ID
prompt. Comparing a value across processes cannot work.

The comparison was also weaker than it looked. The key was made with `.userPresence`, which accepts the device
passcode as well as Face ID, so a person who knows the passcode could approve the key without Face ID at all.

## Decision

- `QK` is made with `.biometryCurrentSet` (and the Secure Enclave, `ThisDeviceOnly`, the shared keychain group)
  instead of `.userPresence`. The system itself makes the key unusable when the enrolled Face ID or Touch ID set
  changes, in every process, and Face ID is the only way to use it: there is no passcode fallback for `QK`.
- No code reads or compares the biometric state any more. `QuickUnlockChip.biometricState()`, the saved state in
  `QuickUnlockPolicy` and the extension baseline of the interim fix are removed. When the key is gone the unlock
  already ends quick unlock with the reason `biometricsChanged` and asks for the master password, as before.
- Quick unlock needs enrolled biometrics (the chip is available only when Face ID or Touch ID can be evaluated). A phone
  with only a passcode uses the master password, as an iPhone without Face ID did already.
- The fallback is the master password, unchanged. The next full unlock turns Face ID on again without a prompt and
  makes a new key for the then-current set.
- The `qu_blob` wrap is unchanged, so the vectors in `quick-unlock.json` still apply and no new vectors are needed.
- A key made by an earlier build with `.userPresence` keeps working until Face ID is turned on again; the next full unlock
  replaces it.

## Consequences

- AutoFill, the app and the extension behave the same, and a changed face set is enforced by the system, not by code.
- Face ID can no longer be approved with the passcode for `QK`. Someone who knows the passcode and adds a face loses
  quick unlock for the old set, and the owner re-enables it with the master password.
- Turning on quick unlock fails on a phone with no enrolled face: the person is told to use the master password.
- One piece of state (`biometricState` in `quick-unlock.json`) is gone from the format. Files that still have it
  decode, the field is ignored.

## Alternatives considered

- **Keep comparing states, with a baseline per process (the interim fix).** Works, but a per-process baseline taken at the
  first use trusts that moment, and the check stays our own code on top of a key that accepts the passcode.
- **Drop the biometric-set rule.** Simplest, but it gives up ADR-0041's intent for a phone.
