# FigLock threat model

Status: draft v1 (FIG-19). Read with `crypto.md` (what the cryptography does), `docs/prd.md` section 5
(non-goals) and `docs/research.md` section 4 (agent access). It states what FigLock defends, against whom,
and where protection stops. Where a mitigation is a test, it is named so it can be traced to a test in the
repo (testing philosophy in `AGENTS.md`).

## 1. Assets

| # | Asset | Why it matters |
| - | ----- | -------------- |
| A1 | Secret values: passwords, API tokens, SSH keys, `.env` bundles, TOTP seeds | direct account and infrastructure access |
| A2 | Item metadata: titles, URLs, usernames, tags, notes | reveals accounts, habits and what to attack |
| A3 | Master password and Secret Key | root of the key hierarchy |
| A4 | Vault Key and derived keys, device private keys | unlock everything, or forge writes |
| A5 | Vault integrity and availability | a silent change or loss of a password is damage |
| A6 | The agent audit log | the user's record of what agents did |
| A7 | Approval decisions (grants) | an attacker who forges one gets secrets |
| A8 | The software supply chain (releases, web app JS, extension, CI) | one bad release reaches everyone |

## 2. Actors

| Actor | Capability assumed |
| ----- | ------------------ |
| Storage provider or anyone with access to the storage account (Google, Apple, a folder host) | read, write, delete, roll back and withhold any file; observe sizes and times |
| Network attacker | observe and modify traffic to Drive, the relay, `figlock.com`, update checks |
| Thief with a locked device | the device, its disk, its sleeping memory image |
| Malware as the same user on an unlocked device | read process memory, keystrokes, clipboard, files, screen |
| Local attacker without the user's login | reads files of other users, shared machine |
| Curious or compromised agent | runs commands as the user, reads transcripts, can be prompt-injected by content it reads |
| Malicious website | tries to trigger autofill, steal credentials, phish approvals |
| Revoked or lost device | holds old keys and a copy of past data |
| Malicious dependency or build contributor | code runs in our build or in our users' apps |
| The relay operator (us, or an impersonator) | sees approval traffic in transit |
| Insider: a future FigLock maintainer | can ship releases |

Out of scope as adversaries: the user's own authorised actions, a fully malicious operating system or
hardware, state actors with physical access and unlimited time, and a user who shares their Emergency Kit.

## 3. Trust boundaries

1. **Device boundary.** Secrets exist in plaintext only in process memory of an unlocked FigLock process.
2. **Storage boundary.** Everything crossing to storage is ciphertext and signed (`crypto.md` 7).
3. **Broker boundary.** Agents talk to the broker daemon over a local socket; they never get the vault
   key. The broker decides whether to use a secret on their behalf.
4. **Relay boundary.** The relay is a pipe for end-to-end encrypted approvals; it holds no state and no keys.
5. **UI boundary.** Approval prompts are drawn by FigLock itself, not by anything an agent controls.

## 4. Threats and mitigations

Status column: **M** mitigated by design, **P** partly mitigated, **A** accepted and stated to users.

### 4.1 Storage and network

| ID | Threat | Mitigation | Status | Test |
| -- | ------ | ---------- | ------ | ---- |
| S1 | Provider reads vault contents | Everything encrypted with keys the provider never sees; names, titles and URLs are inside ciphertext | M | storage fixture contains no plaintext titles or URLs |
| S2 | Provider or thief brute-forces the master password | 128-bit Secret Key in the derivation; Argon2id with floor and default parameters | M | wrong-key and KDF floor vectors |
| S3 | Storage attacker forges or edits ops | Ed25519 signatures by device keys; device records signed by the authority key | M | tampered op rejected; unsigned device rejected |
| S4 | Storage attacker alters KDF parameters or `min_format_version` | `vault.json` signed after unlock; parameters in the `VK` wrap AAD; floor and ceiling enforced | M | altered-parameter vector |
| S5 | Rollback: serve an older version of a device's log or a snapshot | per-device sequence numbers, gap detection, snapshot high-water marks (sync spec) | P | rollback and gap tests (FIG-17) |
| S6 | Withholding or deleting files | Full local copy on every device; clients warn on missing sequence numbers; deletion by the provider is recoverable from any device | P | missing-file convergence test |
| S7 | Metadata leaks: file counts, sizes, times, number of devices | Not hidden. Sizes are rounded up to padding buckets (sync spec); counts are accepted | A | none (documented) |
| S8 | Network attacker intercepts sign-in or traffic | TLS to Google and the relay; OAuth with PKCE; payloads are already end-to-end encrypted | M | n/a |
| S9 | Google account takeover | Attacker gets ciphertext only, same as S1 and S2; can delete the appDataFolder, so local copies are the backup | P | restore-from-local test |

### 4.2 Device

| ID | Threat | Mitigation | Status | Test |
| -- | ------ | ---------- | ------ | ---- |
| D1 | Stolen locked device | Disk encryption is the user's job; vault keys are not stored unwrapped; quick-unlock key sits in the Secure Enclave, Keystore or TPM and needs biometrics | M | no plaintext keys on disk scan |
| D2 | Device left unlocked | Auto-lock on idle, sleep, screen lock; clipboard cleared after 30 s | M | lock-on-idle test |
| D3 | Biometric brute force | Platform limits plus 5 failed attempts force a full unlock | M | lockout test |
| D4 | Memory scraping after lock | Zeroize on drop; keys dropped on lock; `mlock` where available; no core dumps in the daemon | P | zeroization tests, no keys in logs |
| D5 | Swap or crash dumps contain secrets | `mlock`, no dumps, and short exposure windows; cannot be guaranteed on every OS | P | none (documented) |
| D6 | Malware as the user on an unlocked device | Not defended. It can read the unlocked process, the clipboard and the screen | A | none |
| D7 | Keylogger captures the master password | Not defended; quick unlock reduces how often it is typed; Secret Key limits what the password alone gives | A | none |
| D8 | Lost or revoked device keeps old data | Revocation rotates `VK` so nothing new is readable; past data stays readable | P | revoked device cannot open new epoch |
| D9 | Another local user reads vault files | Files are ciphertext; the broker socket is mode 0600 and checks the peer uid | M | socket permission and wrong-uid tests |

### 4.3 Agents and the broker (phase 2)

| ID | Threat | Mitigation | Status | Test |
| -- | ------ | ---------- | ------ | ---- |
| G1 | Agent asks for a secret it should not have | Every grant needs biometric approval naming the agent, item or scope, mode and duration; no standing access; grants expire | M | request without grant refused |
| G2 | Agent reads a token from the chat or transcript | Default mode is `figlock http`: the broker adds the auth header itself, so the value never reaches the agent | M | token absent from transcript fixture |
| G3 | Agent uses an approved token against the wrong host | Host allow-list per token, enforced by the broker including redirects | M | host not on allow-list refused; redirect to other host refused |
| G4 | Secret leaks through command output | `figlock run` scrubs injected values from output (also common encodings: base64, URL-encoded, split across lines) | P | scrubbed-output tests, including encoded forms |
| G5 | Determined agent with shell prints a transformed value in `run` mode | Cannot be stopped: any process that receives an env var can emit it in any form | A | documented limit; broker HTTP is the safe default |
| G6 | Prompt-injected agent asks for broader access than the task needs | The approval prompt shows what is requested in plain words; short default TTL; reveal needs its own approval every time | P | reveal needs fresh approval each time |
| G7 | Agent spoofs another agent's identity | Identity comes from the process chain and socket peer credentials, not from a claimed name; shown in the prompt | P | spoofed name does not change the prompt |
| G8 | Approval fatigue: the user clicks yes without reading | Few prompts, specific wording, grants scoped to a session, loud wording on reveal; a daily summary of grants from the audit log | P | UI review (FIG-23) |
| G9 | Forged approval over the relay | Approvals are signed by the approving device key and bound to a request id and nonce; the relay cannot mint them | M | replayed and forged approval refused |
| G10 | Audit log tampering by an agent | Records are encrypted under `AK`, signed by the device key and hash-chained (`broker.md` 5); without `VK` an agent reads nothing and cannot change, insert, reorder or remove a middle record unnoticed. Deleting the file or its tail is not prevented, because the agent runs as the user | P | tampered, dropped, reordered and wrong-key records detected; no plaintext in the file |
| G11 | Skill files or `--json` output teach agents to leak secrets | The shipped skill instructs agents to use `http` and `run`; the CLI never prints secrets without `reveal` | P | CLI output scan in the end-to-end test (FIG-39) |
| G12 | Another process of the user steals a grant token | A token is a handle, not a secret value; it expires (`once`, 15 min, session, 1 day, until lock), `http` mode keeps values from the holder, hosts are allow-listed, and every use is logged. Tokens are bound to the user, not a process, which is the same trust as the agent holding it | P | expired, consumed and revoked tokens refused |

### 4.4 Web, extension and supply chain

| ID | Threat | Mitigation | Status | Test |
| -- | ------ | ---------- | ------ | ---- |
| W1 | Whoever serves the PWA changes its JavaScript | Open source, immutable deploys, strict CSP, visible build hash; power users are pointed to the extension or desktop app | P | CSP test; build hash shown |
| W2 | XSS in the PWA or extension reads secrets | Strict CSP without inline scripts, framework escaping, no remote code, Trusted Types where supported | P | CSP and escaping tests |
| W3 | Malicious site triggers autofill or reads filled fields | Autofill only on exact registrable-domain match, user gesture required, no autofill in cross-origin iframes by default | P | phishing-domain and iframe tests |
| W4 | Extension update is hijacked | Store signing, two-person release rule when there is a second maintainer, release notes from tooling | P | release checklist |
| X1 | Malicious dependency | `cargo-deny`, `cargo-vet`, minimal dependency list, lockfiles, Dependabot with review | P | CI checks |
| X2 | Compromised CI or release pipeline | Pinned and updated actions, least-privilege tokens, signed and reproducible releases (phase 6), signed Homebrew formula | P | reproducible-build check |
| X3 | Installer script tampering (`figlock.com/install.sh`) | Served over TLS from our domain, verifies checksums and signatures of the binary it fetches | P | install script test |
| X4 | Domain or email takeover (`figlock.com`, `security@`) | Cloudflare DNSSEC and 2FA, registrar lock, auto-renew | P | FIG-11 checks |

### 4.5 Recovery and user error

| ID | Threat | Mitigation | Status | Test |
| -- | ------ | ---------- | ------ | ---- |
| R1 | User loses password and Secret Key | Emergency Kit with a forced "verify you saved it" step; optional Shamir recovery; any paired device can re-issue access | P | recovery flow test |
| R2 | Emergency Kit is stolen | It holds the Secret Key but not the password; the thief still needs the password | P | n/a |
| R3 | Shamir shares gathered by an attacker | Threshold chosen by the user; shares are useless below it; spec in FIG-18 | P | share threshold tests |
| R4 | User exports plaintext and forgets it | Export warns and offers encrypted export first | A | UI test |
| R5 | Accidental plaintext in logs, issues, fixtures | Non-negotiable in `AGENTS.md`; secret scanning with push protection; fixtures use generated values | P | secret-scanning CI |

## 5. What FigLock protects and what it does not

Protects:

- Vault contents from the storage provider, from thieves of a locked device or a copy of the vault, and from
  network attackers.
- Secrets from appearing in agent transcripts, when agents use the broker's HTTP mode.
- Writes from forgery by anyone without a device key, and the vault from silent parameter downgrade.
- Newly written data from a device after it is revoked.

Does not protect:

- Anything on a device already running malware as the user (D6, D7).
- A user whose password and Emergency Kit are both compromised.
- Data a revoked device or a malicious agent already copied.
- File counts, sizes, timestamps and the number of devices (S7).
- Against a determined agent with shell access that has been given a secret by `run` or `reveal` (G5).
- Post-quantum attackers.

These limits MUST appear in user-facing documentation and the security page on `figlock.com` in the same
words. They are not marketing risks; they are the product promise.

## 6. Assumptions

- Platform keystores (Secure Enclave, Android Keystore, TPM) and the OS CSPRNG work as documented.
- RustCrypto implementations of the chosen primitives are correct and constant time.
- The user's device disk is encrypted and their OS account is protected.
- Cloudflare, Google and Apple account security is the user's responsibility; FigLock does not depend on it
  for confidentiality.

## 7. Review and audit

- This document is reviewed at each phase boundary and whenever a spec or trust boundary changes. A change
  to the crypto or sync spec also needs an ADR.
- Every row with a test name MUST have that test before the phase that ships it. Rows with status A MUST
  appear in the user documentation.
- The external audit (FIG-56) receives this document, the specs and the test vectors as its starting scope,
  and its findings are published.

## 8. Open items

- Padding buckets for file sizes (S7): decided in the sync spec (FIG-17).
- Agent identity from the process chain (G7) on Windows: confirm available primitives in phase 2.
- Whether the daily grant summary (G8) is on by default.
