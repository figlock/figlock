# Export compliance for the iPhone app (FIG-175)

Draft for the account holder to review. It records what FigLock's encryption is, what the US export rules (BIS, the
EAR) and Apple's App Store Connect ask for, and what to do about each. It is a reading of published text, not legal
advice. Where a point is not settled, it is marked **Open** and needs a decision or a check by the account holder.

Why this exists: the first signed TestFlight upload was rejected with "Invalid Export Compliance Code", because the app
declares non-exempt encryption (`ITSAppUsesNonExemptEncryption` is `true`) and has no compliance code yet
(`docs/ios-app-store.md`, section 1; `docs/ios-release.md`).

## 1. What the app's cryptography is

All of it is standard, publicly specified cryptography from the RustCrypto crates (`docs/spec/crypto.md`, section 2),
plus what Apple's operating system provides. Nothing is home made. A community scheme used earlier for optional Shamir recovery cards, SLIP-0039, was removed
(ADR-0045, FIG-242) so that every primitive in the product is a published standard.

| Use | Primitive | Specification |
| --- | --- | --- |
| Password hashing | Argon2id | RFC 9106 |
| Key derivation | HKDF-SHA-256, PBKDF2-HMAC | RFC 5869, RFC 8018 |
| Encrypting vault items, logs and snapshots | XChaCha20-Poly1305 | draft-irtf-cfrg-xchacha, RFC 8439 construction |
| Signatures | Ed25519 | RFC 8032 |
| Key agreement (pairing devices) | X25519, P-256 ECDH | RFC 7748, SEC 1 / NIST SP 800-56A |
| Hashes and MACs | SHA-256, HMAC | FIPS 180-4, RFC 2104 |
| Recovery Key | A random X25519 key written as a base32 code with a checksum (no encryption of its own) | RFC 7748 |
| Password breach check (identification only, no encryption) | SHA-1 of a password, first five hex characters only (k-anonymity range query) | FIPS 180-4 |
| Transport to Google Drive | TLS through rustls and the system certificate verifier | RFC 8446 |
| Biometric unlock key | Secure Enclave key through Apple's LocalAuthentication and Security frameworks | provided by iOS |

Purpose: to keep the person's own data confidential (the vault is stored as ciphertext in their own storage), to
check that data was not changed, and to authenticate devices. FigLock has no server and no account, so no data
reaches the project. The app does not let a third party add cryptography, and the user cannot change the algorithms.

## 2. What it means under the US rules (BIS)

Software whose job includes protecting data with cryptography is controlled in Category 5, Part 2 (5D002), unless
it is released by the mass-market rule (Note 3 to Category 5, Part 2) as 5D992.c. Self-classification under
License Exception ENC section 740.17(b)(1) is the route for mass-market software that uses only standard
cryptography; items with non-standard cryptography need a classification request to BIS first (section 740.17(b)(3)).

**Assessment.**

- Standard cryptography only: yes, see section 1 (after SLIP-0039 was removed).
- Mass market: FigLock is a free app anyone can install from the App Store with no customisation, which fits
  Note 3 paragraph (a) ("generally available to the public"). It is not network infrastructure and not in the
  excluded groups of 740.17(b)(2) or (b)(3)(iii). **Open:** confirm the Note 3 criteria against the text (the BIS
  page links a PDF of Note 3 that was not read in this draft).
- Likely classification: **5D992.c**, self-classified under 740.17(b)(1). No licence and no prior submission to BIS.

**Reporting.** Items exported under 740.17(b)(1) need an annual self-classification report, unless a classification
(CCATS) was obtained instead. BIS says the report is due by **1 February** of the following year, in **CSV** format
only (columns in Supplement No. 8 to part 742), emailed to crypt-supp8@bis.doc.gov and enc@nsa.gov, and that none
is needed when nothing was exported in that calendar year. Each product is listed once; a later year that changes
nothing is a short email saying so.

- **Open:** a law-firm note says mass-market 5A992.c and 5D992.c items are exempt from the annual report, which BIS's own
  page does not say. Treat the report as required until confirmed with BIS or counsel.
- **Open:** whether an iOS app counts as the "executable software" that the reporting paragraph (e)(3) lists. The
  definition excludes complete binary images, so this is unclear from the text alone.
- Distribution through the App Store and TestFlight is an export. A TestFlight build in 2026 means a first report is
  due by **1 February 2027** for items exported in 2026.

- **Closed:** SLIP-0039, the one non-standard scheme, was removed in ADR-0045 (FIG-242). The built iPhone library has
  no `slip39` symbols, so the "standard cryptography only" assessment above holds for the whole product. The remaining
  PBKDF2 names in the library are unused error types inside `ring`, the TLS library.

**The public source repository.** The source is public (MIT or Apache-2.0). Publicly available encryption source code
is outside the EAR except for a notice to BIS and the NSA when it contains *non-standard* cryptography (section
742.15(b)). Everything in section 1 is standard, so we believe no notice is needed. **Open:** the part 772
definition of "non-standard cryptography" was not read in this draft; check that none of the primitives falls under it.

## 3. What Apple asks

- Info.plist: `ITSAppUsesNonExemptEncryption` is `true` (encryption beyond the OS). Apple's table treats "an industry
  standard algorithm, not provided within the Apple operating system" as needing documentation, and names only a French
  encryption declaration, required "only if you're distributing your app on the App Store in France".
  (Apple's pages were read in part; verify against the live pages.) Apple does not name a self-classification
  report. Proprietary algorithms would need a CCATS.
- App Store Connect, app FigLock, App Information, App Encryption Documentation, Upload: a dialog in three steps. Step 1
  asks for an "App Purpose" of up to 300 characters; the later steps take the documentation file.
- Once the documentation is approved Apple shows a key value (about two business days). It goes into
  `Info.plist` as `ITSEncryptionExportComplianceCode` so builds stop asking the questions. In CI this is the
  repository variable `ITS_EXPORT_COMPLIANCE_CODE` (`docs/ios-release.md`).

**Open:** whether Apple accepts the questionnaire with no file when the app is not available in France, or still wants
a document. The dialog will say after step 1. Do not guess: answer it and see.

### Text for "App Purpose" (under 300 characters)

> FigLock is a password and secrets manager. It encrypts the user's vault on the device with standard algorithms
> (XChaCha20-Poly1305, Argon2id, X25519, Ed25519) and stores only ciphertext in the user's own cloud or folder. No server of ours.

(Check the length when pasting: the limit is 300.)

## 4. Recommended path

1. Decide on France. Either leave France out of the app's availability for now (Pricing and Availability) or file the
   French declaration before distributing there. Leaving it out is the quickest and can be reversed later.
2. Start the App Encryption Documentation dialog in App Store Connect with the text above and see what it asks next. Add
   a short, factual document if it asks for one (section 1 and the primitives table are written to serve as that
   description). Do not upload a document you have not read and agree with.
3. When Apple shows the key value, set the repository variable `ITS_EXPORT_COMPLIANCE_CODE`.
4. Confirm the classification (5D992.c by self-classification) and the
   reporting duty with BIS or an export counsel; the questions marked **Open** above are the list to bring.
5. Put **1 February 2027** in the calendar: the first self-classification report (CSV, FigLock listed once) if counsel
   confirms the duty, covering the 2026 TestFlight builds. Keep a copy.
6. Revisit this document when a primitive changes (`docs/spec/crypto.md` needs an ADR for that anyway), when Android
   or other stores are added, and each January.

## 5. Sources

- Apple, determining and uploading app encryption documentation, and the export compliance documentation table, in
  the App Store Connect Help.
- BIS, encryption controls: mass market, and annual self-classification (bis.gov).
- 15 CFR 740.17 (License Exception ENC) and 742.15 (publicly available encryption source code), via law.cornell.edu.
- The BIS rule of 29 March 2021 amended both sections; its text was not read for this draft. **Open:** read it, as it
  is the most recent change to the reporting rules.
