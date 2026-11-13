# Test vectors

Vectors for every rule in `crypto.md`, `sync.md` and `pairing-recovery.md` (FIG-20). Each file is JSON:

```json
{ "format": "figlock-vectors-1", "title": "...", "cases": [
  { "name": "...", "spec": "crypto.md 6.1", "inputs": { "...": "hex or plain values" }, "expected": { "...": "hex" } },
  { "name": "...", "spec": "...", "inputs": { }, "expected": "reject", "why": "reason, for humans only" }
] }
```

- Binary values are lower case hex. `expected: "reject"` means the operation MUST fail with the one generic
  error (`crypto.md` 10), never partially succeed. The `why` text is not part of the contract.
- Positive cases that produce output (sealing, signing, derivation) list every input including the nonce, so
  an implementation reproduces the bytes exactly.
- All keys, passwords and ids are fixed test values derived from labels (`fill` in `generate.py`). No real
  secrets are used anywhere (`AGENTS.md`).

| File | Covers |
| ---- | ------ |
| `secret-key.json` | Secret Key encode, decode, confusables, check mismatch |
| `kdf.json` | Argon2id default and floor, NFKD, floor and ceiling rejection |
| `key-hierarchy.json` | `sk_key`, `AUK`, derived keys, `VK` wrap and unwrap, tamper cases |
| `items.json` | per-item key wrap and per-field seal and open, replay cases |
| `files.json` | op file and snapshot container: build, read, signature, header, epoch |
| `signatures.json` | device record and `vault.json` signatures, downgrade detection |
| `rotation.json` | epoch key chain and what rotation changes |
| `quick-unlock.json` | quick unlock blob, software-key variant and the Secure Enclave wrap (ECDH, HKDF, seal) |
| `pairing.json` | invite code, keys, request, response, wrong code, swapped key |
| `seal-to.json` | Recovery Key `seal_to`, rewrap needing only the public key |
| `slip39.json` | 2-of-3 and 3-of-5 SLIP-0039 shares of a Recovery Key (generated once) |
| `slip39-official.json` | the 45 official SLIP-0039 vectors, unchanged from upstream (ADR-0028); run by the Rust tests, not regenerated |
| `hlc.json` | hybrid logical clock rules, overflow, drift hold |
| `merge.json` | field merge, delete versus restore, snapshot equivalence, purge horizon |
| `audit.json` | audit key `AK`, audit record container and hash chain (broker log) |
| `sync-reader.json` | expected outcomes of the reader state machine (gap, fork, rollback, revoked); checked by the Rust implementation, not recomputable here |

## How they are made and checked

`generate.py` uses `reference.py`, a small Python implementation written directly from the specs and
independent of the Rust core. The Rust core (FIG-24, FIG-26) must reproduce every positive case byte for
byte and reject every negative one. Property tests for convergence live with the Rust code (`sync.md` 12).

```sh
just vectors-check   # create a venv, install pinned requirements, regenerate in memory and compare
just vectors         # rewrite the JSON files (only when a spec rule changes)
```

`--check` fails if a committed file differs from the regenerated output (drift), if any case does not
recompute, if a negative case is accepted, and if the primitives in `reference.py` do not match the published
RFC 5869, RFC 8032, RFC 7748 and XChaCha20-Poly1305 draft vectors. CI runs it whenever this folder or the
specs change.

## Rules

- A change to a crypto or sync rule needs an ADR, a spec edit, and regenerated vectors in the same PR
  (`AGENTS.md`).
- `slip39.json` is generated once because the library shares use real randomness; `--check` verifies it
  (every threshold combination recovers, below threshold fails, a corrupted word fails the checksum) and never
  rewrites it. Delete it only when the format of shares changes.
- Besides these, the Rust implementation MUST also run the official SLIP-0039 vectors from the upstream
  `trezor/python-shamir-mnemonic` repository and the RFC 9106 Argon2 vectors through its own crates.
- The Argon2id wrapper fixes type Id, version 0x13 and no secret or associated data; the library's own RFC
  9106 tests cover the algorithm.
