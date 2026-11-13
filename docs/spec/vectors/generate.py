#!/usr/bin/env python3
"""Generate or check the FigLock test vectors (FIG-20).

    python3 generate.py            write the JSON files next to this script
    python3 generate.py --check    regenerate in memory and fail if any committed file differs,
                                   and re-verify every vector including the negative cases

All "random" inputs are derived from fixed labels (see `fill`), so output is byte-for-byte reproducible.
The SLIP-0039 shares (slip39.json) use real randomness in the library, so they are generated once and then
only verified, never regenerated.
"""
from __future__ import annotations

import itertools
import json
import pathlib
import random
import sys

import cbor2
import reference as r

HERE = pathlib.Path(__file__).parent


def fill(label: str, n: int) -> bytes:
    """Deterministic test input of n bytes. Not part of the spec; just fixed bytes for vectors."""
    out = b""
    i = 0
    while len(out) < n:
        out += r.sha256(f"figlock-vector/{label}/{i}".encode())
        i += 1
    return out[:n]


def x(b: bytes) -> str:
    return b.hex()


def flip(b: bytes, i: int = -1) -> bytes:
    b = bytearray(b)
    b[i] ^= 1
    return bytes(b)


VAULT_ID = fill("vault-id", 16)
DEVICE_A = fill("device-a", 16)
DEVICE_B = fill("device-b", 16)
ITEM_ID = fill("item-id", 16)
SK = fill("secret-key", 16)
SALT = fill("salt", 16)
VK = fill("vault-key", 32)
PASSWORD = "correct horse battery staple"
DEFAULT = (65536, 3, 4)
FLOOR = r.FLOOR


def case(name: str, spec: str, inputs: dict, expected, **extra) -> dict:
    return {"name": name, "spec": spec, "inputs": inputs, "expected": expected, **extra}


def reject(name: str, spec: str, inputs: dict, why: str) -> dict:
    return {"name": name, "spec": spec, "inputs": inputs, "expected": "reject", "why": why}


# ---------------------------------------------------------------------------------------------
def secret_key():
    cases = []
    text = r.secret_key_format(SK)
    cases.append(case("encode", "crypto.md 4.1", {"sk": x(SK)}, {"text": text}))
    cases.append(case("decode-lowercase-and-spaces", "crypto.md 4.1", {"text": text.lower().replace("-", " ")},
                      {"sk": x(SK)}))
    confusable = "FL1-" + text[4:].replace("0", "O").replace("1", "I")
    cases.append(case("decode-confusables", "crypto.md 4.1", {"text": confusable}, {"sk": x(SK)},
                      note="O reads as 0, I and L read as 1 (Crockford)"))
    chars = text.replace("-", "")
    bad = chars[:-1] + ("0" if chars[-1] != "0" else "1")
    cases.append(reject("check-mismatch", "crypto.md 4.1", {"text": "FL1-" + bad[3:]},
                        "typo in Secret Key, reported as such and not as a wrong password"))
    cases.append(reject("wrong-length", "crypto.md 4.1", {"text": text[:-5]}, "length"))
    return {"title": "Secret Key encoding", "cases": cases}


def recovery_key():
    cases = []
    rk = fill("rk-priv", 32)
    text = r.recovery_key_format(rk)
    cases.append(case("encode", "crypto.md 4.2", {"rk_priv": x(rk)}, {"text": text}))
    cases.append(case("decode-lowercase-and-spaces", "crypto.md 4.2", {"text": text.lower().replace("-", " ")},
                      {"rk_priv": x(rk)}))
    confusable = "FLR1-" + text[5:].replace("0", "O").replace("1", "I")
    cases.append(case("decode-confusables", "crypto.md 4.2", {"text": confusable}, {"rk_priv": x(rk)},
                      note="O reads as 0, I and L read as 1 (Crockford); the FLR1 prefix is matched first"))
    chars = text.replace("-", "")
    bad = chars[:-1] + ("0" if chars[-1] != "0" else "1")
    cases.append(reject("check-mismatch", "crypto.md 4.2", {"text": "FLR1-" + bad[4:]},
                        "typo in Recovery Key, reported as such"))
    cases.append(reject("wrong-length", "crypto.md 4.2", {"text": text[:-5]}, "length"))
    cases.append(reject("secret-key-is-not-a-recovery-key", "crypto.md 4.2", {"text": r.secret_key_format(SK)},
                        "the FL1 prefix of a Secret Key is not FLR1"))
    return {"title": "Recovery Key encoding", "cases": cases}


def kdf():
    cases = []
    for name, (m, t, p) in (("default", DEFAULT), ("floor", FLOOR)):
        out = r.pw_key(PASSWORD, SALT, m, t, p)
        cases.append(case(f"argon2id-{name}", "crypto.md 5.1",
                          {"password": PASSWORD, "salt": x(SALT), "m_kib": m, "t": t, "p": p}, {"pw_key": x(out)}))
    nfd = "é"  # NFD form of e-acute
    nfc = "é"
    k1 = r.pw_key("pass" + nfd, SALT, *FLOOR)
    k2 = r.pw_key("pass" + nfc, SALT, *FLOOR)
    assert k1 == k2
    cases.append(case("nfkd-normalisation", "crypto.md 3", {"passwords_equal_after_nfkd": ["pass" + nfd, "pass" + nfc],
                                                           "salt": x(SALT), "m_kib": FLOOR[0], "t": FLOOR[1],
                                                           "p": FLOOR[2]}, {"pw_key": x(k1)}))
    for name, params in (("m-below-floor", (32767, 2, 1)), ("t-below-floor", (32768, 1, 1)),
                         ("m-above-ceiling", (1048577, 3, 4)), ("t-above-ceiling", (65536, 11, 4)),
                         ("p-above-ceiling", (65536, 3, 17))):
        cases.append(reject(name, "crypto.md 5.1", {"m_kib": params[0], "t": params[1], "p": params[2]},
                            "outside floor or ceiling: client refuses to create or accept"))
    return {"title": "Argon2id parameters and password key", "cases": cases}


def key_hierarchy():
    cases = []
    m, t, p = DEFAULT
    pk = r.pw_key(PASSWORD, SALT, m, t, p)
    sk_k = r.sk_key(SK, VAULT_ID)
    auk = r.auk(PASSWORD, SK, VAULT_ID, SALT, m, t, p)
    cases.append(case("sk-key-and-auk", "crypto.md 5",
                      {"password": PASSWORD, "sk": x(SK), "vault_id": x(VAULT_ID), "salt": x(SALT), "m_kib": m,
                       "t": t, "p": p}, {"pw_key": x(pk), "sk_key": x(sk_k), "auk": x(auk)}))
    # a different Secret Key changes AUK even with the same password
    auk2 = r.auk(PASSWORD, fill("other-sk", 16), VAULT_ID, SALT, m, t, p)
    assert auk2 != auk
    cases.append(case("different-secret-key-different-auk", "crypto.md 5",
                      {"password": PASSWORD, "sk": x(fill("other-sk", 16)), "vault_id": x(VAULT_ID), "salt": x(SALT),
                       "m_kib": m, "t": t, "p": p}, {"auk": x(auk2)}))
    d = r.derive_from_vk(VK)
    cases.append(case("derive-from-vk", "crypto.md 6", {"vk": x(VK)},
                      {"lk": x(d["lk"]), "wk": x(d["wk"]), "authority_seed": x(d["authority_seed"]),
                       "authority_pub": x(d["authority_pub"])}))
    params = r.kdf_params_bytes(m, t, p, SALT)
    nonce = fill("nonce-vk-wrap", 24)
    wrapped = r.wrap_vk(auk, VK, VAULT_ID, 1, params, nonce)
    cases.append(case("vk-wrap", "crypto.md 6.1",
                      {"auk": x(auk), "vk": x(VK), "vault_id": x(VAULT_ID), "key_epoch": 1,
                       "kdf_params_bytes": x(params), "nonce": x(nonce)}, {"wrapped_vk": x(wrapped)}))
    base = {"auk": x(auk), "wrapped_vk": x(wrapped), "vault_id": x(VAULT_ID), "key_epoch": 1,
            "kdf_params_bytes": x(params)}
    cases.append(case("vk-unwrap", "crypto.md 6.1", base, {"vk": x(VK)}))
    cases.append(reject("vk-unwrap-wrong-auk", "crypto.md 6.1", {**base, "auk": x(fill("wrong-auk", 32))},
                        "wrong password or Secret Key"))
    wrong_params = r.kdf_params_bytes(m, t, p, flip(SALT))
    cases.append(reject("vk-unwrap-altered-kdf-params", "crypto.md 5.1, 6.1",
                        {**base, "kdf_params_bytes": x(wrong_params)}, "parameters are in the AAD"))
    cases.append(reject("vk-unwrap-wrong-epoch", "crypto.md 6.1", {**base, "key_epoch": 2}, "epoch is in the AAD"))
    cases.append(reject("vk-unwrap-tampered-tag", "crypto.md 3", {**base, "wrapped_vk": x(flip(wrapped))},
                        "authentication tag"))
    return {"title": "Key hierarchy", "cases": cases}


def items():
    cases = []
    d = r.derive_from_vk(VK)
    ik = fill("item-key", 32)
    n1, n2 = fill("nonce-ik", 24), fill("nonce-field", 24)
    wik = r.wrap_ik(d["wk"], ik, VAULT_ID, ITEM_ID, 1, n1)
    cases.append(case("item-key-wrap", "crypto.md 7.1",
                      {"wk": x(d["wk"]), "ik": x(ik), "vault_id": x(VAULT_ID), "item_id": x(ITEM_ID),
                       "key_epoch": 1, "nonce": x(n1)}, {"wrapped_ik": x(wik)}))
    cases.append(reject("item-key-unwrap-wrong-item", "crypto.md 7.1",
                        {"wk": x(d["wk"]), "wrapped_ik": x(wik), "vault_id": x(VAULT_ID),
                         "item_id": x(fill("other-item", 16)), "key_epoch": 1}, "item id is in the AAD"))
    hlc = r.hlc_bytes(1_700_000_000_000, 3)
    value = cbor2.dumps("Cloudflare", canonical=True)
    blob = r.field_blob(ik, value, VAULT_ID, ITEM_ID, "title", hlc, n2)
    inputs = {"ik": x(ik), "value_cbor": x(value), "vault_id": x(VAULT_ID), "item_id": x(ITEM_ID),
              "field_id": "title", "hlc": x(hlc), "nonce": x(n2)}
    cases.append(case("field-seal", "crypto.md 7.1", inputs, {"field_blob": x(blob)}))
    o = {"ik": x(ik), "field_blob": x(blob), "vault_id": x(VAULT_ID), "item_id": x(ITEM_ID), "field_id": "title",
         "hlc": x(hlc)}
    cases.append(case("field-open", "crypto.md 7.1", o, {"value_cbor": x(value)}))
    cases.append(reject("field-open-wrong-field", "crypto.md 7.1", {**o, "field_id": "username"},
                        "field id is in the AAD"))
    cases.append(reject("field-open-wrong-hlc", "crypto.md 7.1", {**o, "hlc": x(r.hlc_bytes(1_700_000_000_000, 4))},
                        "stamp is in the AAD: no replay under another stamp"))
    cases.append(reject("field-open-wrong-item", "crypto.md 7.1", {**o, "item_id": x(fill("other-item", 16))},
                        "item id is in the AAD"))
    cases.append(reject("field-open-tampered", "crypto.md 3", {**o, "field_blob": x(flip(blob))}, "tag"))
    cases.append(reject("field-open-unknown-version", "crypto.md 12",
                        {**o, "field_blob": x(b"\x02" + blob[1:])}, "unsupported envelope version"))
    return {"title": "Per-item keys and field encryption", "cases": cases}


def padded_payload(doc: dict, bucket: int = 1024) -> bytes:
    """Canonical CBOR with a zero `pad` member sized so the total is exactly `bucket` bytes (sync.md 11)."""
    base = len(cbor2.dumps({**doc, "pad": b""}, canonical=True))
    for n in range(bucket - base + 1):
        out = cbor2.dumps({**doc, "pad": b"\x00" * n}, canonical=True)
        if len(out) == bucket:
            return out
    raise AssertionError("no pad length fits the bucket")


def payload_op(seq: int, prev: bytes, ops: list) -> bytes:
    return padded_payload({"v": 1, "device": DEVICE_A, "seq": seq, "prev": prev, "ops": ops})


def files():
    cases = []
    d = r.derive_from_vk(VK)
    sign_seed = fill("device-a-sign", 32)
    sign_pub = bytes(r.SigningKey(sign_seed).verify_key)
    ops1 = [{"op": "item.create", "item": ITEM_ID, "hlc": r.hlc_bytes(1_700_000_000_000, 0), "wrapped_ik": b"\x00" * 8}]
    payload = payload_op(1, b"\x00" * 32, ops1)
    nonce = fill("nonce-file", 24)
    f = r.build_file(r.KIND_OP, d["lk"], sign_seed, VAULT_ID, DEVICE_A, 1, 1, payload, nonce)
    base = {"lk": x(d["lk"]), "device_sign_pub": x(sign_pub), "vault_id": x(VAULT_ID), "device_id": x(DEVICE_A),
            "seq": 1, "kind": "op"}
    cases.append(case("op-file-build", "sync.md 4, crypto.md 7.2-7.3",
                      {**base, "device_sign_seed": x(sign_seed), "key_epoch": 1, "payload_cbor": x(payload),
                       "nonce": x(nonce)}, {"file": x(f)}))
    cases.append(case("op-file-read", "sync.md 4", {**base, "file": x(f)}, {"payload_cbor": x(payload)}))
    cases.append(reject("op-file-no-device-record", "sync.md 4, 8", {**base, "file": x(f), "device_sign_pub": None},
                        "device has no valid, non-revoked record"))
    cases.append(reject("op-file-flipped-envelope-bit", "crypto.md 7.3", {**base, "file": x(flip(f, 20))},
                        "signature fails before decryption"))
    cases.append(reject("op-file-flipped-signature-bit", "crypto.md 7.3", {**base, "file": x(flip(f, -1))},
                        "signature"))
    cases.append(reject("op-file-wrong-seq-in-path", "sync.md 4", {**base, "file": x(f), "seq": 2},
                        "seq is in the signature input and the AAD"))
    cases.append(reject("op-file-wrong-device-in-path", "sync.md 4",
                        {**base, "file": x(f), "device_id": x(DEVICE_B)}, "device id is in the signature and AAD"))
    other_pub = bytes(r.SigningKey(fill("device-b-sign", 32)).verify_key)
    cases.append(reject("op-file-signed-by-other-device", "crypto.md 7.3",
                        {**base, "file": x(f), "device_sign_pub": x(other_pub)}, "signature"))
    header = bytearray(f[:8])
    header[7] ^= 1  # change key_epoch
    swapped = bytes(header) + f[8:]
    cases.append(reject("op-file-epoch-in-header-tampered", "sync.md 4", {**base, "file": x(swapped)},
                        "epoch is covered by the signature and the AAD"))
    cases.append(reject("op-file-as-snapshot", "sync.md 4", {**{k: v for k, v in base.items() if k != "kind"},
                                                              "kind": "snapshot", "file": x(f)},
                        "kind must match what the reader expects"))
    snap = padded_payload({"v": 1, "device": DEVICE_A, "seq": 1, "hw": {DEVICE_A: 1},
                           "purge_horizon": b"\x00" * 8, "items": []})
    sf = r.build_file(r.KIND_SNAP, d["lk"], sign_seed, VAULT_ID, DEVICE_A, 1, 1, snap, fill("nonce-snap", 24))
    cases.append(case("snapshot-file-build", "sync.md 7",
                      {**{k: v for k, v in base.items() if k != "kind"}, "kind": "snapshot",
                       "device_sign_seed": x(sign_seed), "key_epoch": 1, "payload_cbor": x(snap),
                       "nonce": x(fill("nonce-snap", 24))}, {"file": x(sf)}))
    return {"title": "Op file and snapshot container", "cases": cases}


def audit():
    cases = []
    d = r.derive_from_vk(VK)
    ak = d["ak"]
    sign_seed = fill("device-a-sign", 32)
    sign_pub = bytes(r.SigningKey(sign_seed).verify_key)
    item_hex = x(ITEM_ID)
    t0 = 1_700_000_100_000
    entries = [
        ("grant.requested", t0, {"peer": {"uid": 501, "pid": 4242, "exe": "/usr/bin/example-agent"},
                                 "label": "example agent", "reason": "deploy the site",
                                 "scope": f"item:{item_hex}", "mode": "http", "ttl": "15m"}),
        ("grant.approved", t0 + 4_000, {"peer": {"uid": 501, "pid": 4242, "exe": "/usr/bin/example-agent"},
                                        "grant": x(fill("grant-id", 8)), "scope": f"item:{item_hex}",
                                        "mode": "http", "ttl": "15m"}),
        ("use.refused", t0 + 9_000, {"grant": x(fill("grant-id", 8)), "item": item_hex, "mode": "http",
                                     "host": "evil.example", "code": "not_allowed"}),
    ]
    base = {"ak": x(ak), "device_sign_pub": x(sign_pub), "vault_id": x(VAULT_ID), "device_id": x(DEVICE_A)}

    cases.append(case("audit-key", "crypto.md 6", {"vk": x(VK)}, {"ak": x(ak)}))

    files_, prev, payloads = [], r.AUDIT_ZERO, []
    for n, (ev, t, det) in enumerate(entries, start=1):
        payload = r.audit_payload(prev, t, ev, det)
        nonce = fill(f"nonce-audit-{n}", 24)
        f = r.build_file(r.KIND_AUDIT, ak, sign_seed, VAULT_ID, DEVICE_A, n, 1, payload, nonce)
        cases.append(case(f"audit-payload-{n}", "broker.md 5",
                          {"prev": x(prev), "t": t, "ev": ev, "d": det}, {"payload_jcs": payload.decode()}))
        cases.append(case(f"audit-record-{n}-build", "crypto.md 7.5",
                          {**base, "device_sign_seed": x(sign_seed), "seq": n, "key_epoch": 1,
                           "payload_jcs": payload.decode(), "nonce": x(nonce)}, {"file": x(f)}))
        cases.append(case(f"audit-record-{n}-read", "crypto.md 7.5",
                          {**base, "kind": "audit", "seq": n, "file": x(f)}, {"payload_jcs": payload.decode()}))
        files_.append(f)
        payloads.append(payload)
        prev = r.sha256(f)
    chain = {**base, "files": [x(f) for f in files_]}
    cases.append(case("audit-chain-three-records", "broker.md 5", chain,
                      {"entries": 3, "head": x(r.sha256(files_[-1]))}))
    cases.append(case("audit-chain-empty", "broker.md 5", {**base, "files": []},
                      {"entries": 0, "head": x(r.AUDIT_ZERO)}))

    f1 = files_[0]
    one = {**base, "kind": "audit", "seq": 1, "file": x(f1)}
    cases.append(reject("audit-record-read-with-log-key", "crypto.md 7.5",
                        {**one, "ak": x(d["lk"])}, "the op log key cannot open audit records"))
    cases.append(reject("audit-record-flipped-bit", "crypto.md 7.3", {**one, "file": x(flip(f1, 20))},
                        "signature fails before decryption"))
    cases.append(reject("audit-record-wrong-seq", "crypto.md 7.5", {**one, "seq": 2},
                        "seq is in the signature input and the AAD"))
    cases.append(reject("audit-record-no-device-record", "crypto.md 7.3", {**one, "device_sign_pub": None},
                        "device has no valid record"))
    cases.append(reject("audit-record-read-as-op", "sync.md 4", {**one, "kind": "op"},
                        "kind must match what the reader expects"))
    op_as_audit = r.build_file(r.KIND_OP, ak, sign_seed, VAULT_ID, DEVICE_A, 1, 1, payloads[0],
                               fill("nonce-audit-1", 24))
    cases.append(reject("op-file-read-as-audit", "sync.md 4", {**one, "file": x(op_as_audit)},
                        "kind must match what the reader expects"))

    cases.append(reject("audit-chain-dropped-middle", "broker.md 5",
                        {**chain, "files": [x(files_[0]), x(files_[2])]}, "seq and chain must be continuous"))
    cases.append(reject("audit-chain-reordered", "broker.md 5",
                        {**chain, "files": [x(files_[1]), x(files_[0])]}, "seq is part of every record"))
    cases.append(reject("audit-chain-duplicated", "broker.md 5",
                        {**chain, "files": [x(files_[0]), x(files_[0])]}, "seq is part of every record"))
    bad_prev = r.build_file(r.KIND_AUDIT, ak, sign_seed, VAULT_ID, DEVICE_A, 2, 1,
                            r.audit_payload(r.AUDIT_ZERO, entries[1][1], entries[1][0], entries[1][2]), fill("nonce-x", 24))
    cases.append(reject("audit-chain-wrong-prev", "broker.md 5",
                        {**chain, "files": [x(files_[0]), x(bad_prev)]},
                        "a validly signed record whose prev does not match the one before it"))
    first_prev = r.build_file(r.KIND_AUDIT, ak, sign_seed, VAULT_ID, DEVICE_A, 1, 1,
                              r.audit_payload(r.sha256(b"x"), t0, entries[0][0], entries[0][2]), fill("nonce-y", 24))
    cases.append(reject("audit-chain-first-prev-not-zero", "broker.md 5",
                        {**chain, "files": [x(first_prev)]}, "the first record chains to 32 zero bytes"))
    spaced = json.dumps({"v": 1, "prev": x(r.AUDIT_ZERO), "t": t0, "ev": entries[0][0], "d": entries[0][2]},
                        sort_keys=True).encode()
    non_canonical = r.build_file(r.KIND_AUDIT, ak, sign_seed, VAULT_ID, DEVICE_A, 1, 1, spaced, fill("nonce-z", 24))
    cases.append(reject("audit-chain-non-canonical-payload", "broker.md 5",
                        {**chain, "files": [x(non_canonical)]}, "payload must be exactly its RFC 8785 form"))
    extra = r.jcs({"v": 1, "prev": x(r.AUDIT_ZERO), "t": t0, "ev": "grant.requested", "d": {}, "x": 1})
    extra_f = r.build_file(r.KIND_AUDIT, ak, sign_seed, VAULT_ID, DEVICE_A, 1, 1, extra, fill("nonce-w", 24))
    cases.append(reject("audit-chain-unknown-member", "broker.md 5",
                        {**chain, "files": [x(extra_f)]}, "only v, prev, t, ev and d"))
    return {"title": "Audit key, audit records and their hash chain", "cases": cases}


def signatures():
    cases = []
    d = r.derive_from_vk(VK)
    sign_pub = bytes(r.SigningKey(fill("device-a-sign", 32)).verify_key)
    agree_pub = r.x25519_pub(fill("device-a-agree", 32))
    record = {"device_id": x(DEVICE_A), "name": "Work MacBook", "sign_pub": x(sign_pub), "agree_pub": x(agree_pub),
              "added_at": 1_700_000_000_000, "epoch_added": 1}
    sig = r.sign_json(d["authority_seed"], "device-record", record)
    cases.append(case("device-record-sign", "crypto.md 7.4",
                      {"authority_seed": x(d["authority_seed"]), "record": record,
                       "jcs": r.jcs(record).decode()}, {"signature": x(sig), "authority_pub": x(d["authority_pub"])}))
    v = {"authority_pub": x(d["authority_pub"]), "record": record, "signature": x(sig)}
    cases.append(case("device-record-verify", "crypto.md 7.4", v, "valid"))
    cases.append(reject("device-record-altered-name", "crypto.md 7.4", {**v, "record": {**record, "name": "Evil"}},
                        "record is signed in canonical form"))
    cases.append(reject("device-record-bad-signature", "crypto.md 7.4", {**v, "signature": x(flip(sig))}, "signature"))
    vault_json = {"vault_id": x(VAULT_ID), "revision": 1, "key_epoch": 1, "min_format_version": 1, "suite": "fl1",
                  "kdf": {"m_kib": 65536, "t": 3, "p": 4, "salt": x(SALT)}}
    vsig = r.sign_json(d["authority_seed"], "vault-json", vault_json)
    cases.append(case("vault-json-sign", "crypto.md 7.4",
                      {"authority_seed": x(d["authority_seed"]), "vault_json": vault_json,
                       "jcs": r.jcs(vault_json).decode()}, {"signature": x(vsig)}))
    vv = {"authority_pub": x(d["authority_pub"]), "vault_json": vault_json, "signature": x(vsig), "context": "vault-json"}
    cases.append(case("vault-json-verify", "crypto.md 7.4", vv, "valid"))
    rotated = {**vault_json, "revision": 2, "key_epoch": 2,
               "revoked": [{"device_id": x(DEVICE_B), "sign_pub": x(b"\x01" * 32), "revoked_at": [1_700_000_100_000, 0],
                            "epoch": 2}],
               "epoch_keys": [{"from": 1, "to": 2, "sealed": "00" * 8}]}
    rsig = r.sign_json(d["authority_seed"], "vault-json", rotated)
    cases.append(case("vault-json-sign-with-revocation", "crypto.md 9.2",
                      {"authority_seed": x(d["authority_seed"]), "vault_json": rotated,
                       "jcs": r.jcs(rotated).decode()}, {"signature": x(rsig)}))
    cases.append(reject("vault-json-revoked-list-removed", "crypto.md 9.2",
                        {"authority_pub": x(d["authority_pub"]), "vault_json": {k: v for k, v in rotated.items()
                                                                               if k != "revoked"},
                         "signature": x(rsig), "context": "vault-json"},
                        "dropping the revoked list from a signed vault.json is detected"))
    lowered = {**vault_json, "kdf": {**vault_json["kdf"], "m_kib": 32768}}
    cases.append(reject("vault-json-kdf-params-altered", "crypto.md 5.1, 7.4", {**vv, "vault_json": lowered},
                        "downgrade of KDF parameters is detected after unlock"))
    lowered_ver = {**vault_json, "min_format_version": 0}
    cases.append(reject("vault-json-min-format-version-lowered", "crypto.md 12", {**vv, "vault_json": lowered_ver},
                        "lowering min_format_version is detected"))
    cases.append(reject("vault-json-wrong-context", "crypto.md 7.4", {**vv, "context": "device-record"},
                        "context string separates signature domains"))
    return {"title": "Device records and vault.json signatures", "cases": cases}


def rotation():
    cases = []
    vk2 = fill("vault-key-2", 32)
    nonce = fill("nonce-epoch", 24)
    ek = r.epoch_key(vk2, VK, VAULT_ID, 1, 2, nonce)
    cases.append(case("epoch-key-wrap", "crypto.md 9.1",
                      {"new_vk": x(vk2), "old_vk": x(VK), "vault_id": x(VAULT_ID), "old_epoch": 1, "new_epoch": 2,
                       "nonce": x(nonce)}, {"epoch_key": x(ek)}))
    old = r.open_(vk2, ek, r.aad("epoch-key", VAULT_ID, r.u32(1), r.u32(2)))
    assert old == VK
    cases.append(case("epoch-chain-opens-old-vk", "crypto.md 9.1, sync.md 9",
                      {"new_vk": x(vk2), "epoch_key": x(ek), "vault_id": x(VAULT_ID), "old_epoch": 1,
                       "new_epoch": 2}, {"old_vk": x(VK)}))
    cases.append(reject("epoch-chain-wrong-epochs", "crypto.md 9.1",
                        {"new_vk": x(vk2), "epoch_key": x(ek), "vault_id": x(VAULT_ID), "old_epoch": 1,
                         "new_epoch": 3}, "epochs are in the AAD"))
    d_old, d_new = r.derive_from_vk(VK), r.derive_from_vk(vk2)
    assert d_old["lk"] != d_new["lk"] and d_old["authority_pub"] != d_new["authority_pub"]
    cases.append(case("rotation-changes-derived-keys", "crypto.md 9.1", {"old_vk": x(VK), "new_vk": x(vk2)},
                      {"old_lk": x(d_old["lk"]), "new_lk": x(d_new["lk"]),
                       "old_authority_pub": x(d_old["authority_pub"]), "new_authority_pub": x(d_new["authority_pub"])}))
    return {"title": "Vault Key rotation", "cases": cases}


def quick_unlock():
    qk = fill("quick-unlock-key", 32)
    auk = r.auk(PASSWORD, SK, VAULT_ID, SALT, *FLOOR)
    nonce = fill("nonce-qu", 24)
    blob = r.quick_unlock_blob(qk, auk, VAULT_ID, DEVICE_A, nonce)
    base = {"qk": x(qk), "blob": x(blob), "vault_id": x(VAULT_ID), "device_id": x(DEVICE_A)}
    return {"title": "Quick unlock blob (software-key variant)", "cases": [
        case("quick-unlock-seal", "crypto.md 8",
             {"qk": x(qk), "auk": x(auk), "vault_id": x(VAULT_ID), "device_id": x(DEVICE_A), "nonce": x(nonce)},
             {"blob": x(blob)}, kdf={"m_kib": FLOOR[0], "t": FLOOR[1], "p": FLOOR[2], "salt": x(SALT)}),
        case("quick-unlock-open", "crypto.md 8", base, {"auk": x(auk)}),
        reject("quick-unlock-other-device", "crypto.md 8", {**base, "device_id": x(DEVICE_B)},
               "device id is in the AAD"),
    ] + quick_unlock_se(auk)}


def quick_unlock_se(auk):
    """Secure Enclave variant (ADR-0025): ECDH with QK, HKDF, then the usual seal. QK's secret is a software
    stand-in here; the product keeps it in the Secure Enclave."""
    qk_secret = fill("qk-se-secret", 32)
    qk_pub = r.p256_pub(qk_secret)
    eph_secret = fill("qk-se-eph", 32)
    other_secret = fill("qk-se-other", 32)
    nonce = fill("nonce-qu-se", 24)
    eph_pub = r.p256_pub(eph_secret)
    shared = r.p256_agree(eph_secret, qk_pub)
    assert shared == r.p256_agree(qk_secret, eph_pub)
    wrap_key = r.se_wrap_key(shared, eph_pub, qk_pub, VAULT_ID, DEVICE_A)
    blob = r.quick_unlock_se_blob(qk_pub, eph_secret, auk, VAULT_ID, DEVICE_A, nonce)
    base = {"qk_secret": x(qk_secret), "blob": x(blob), "vault_id": x(VAULT_ID), "device_id": x(DEVICE_A)}
    flipped = bytearray(blob)
    flipped[-1] ^= 1
    swapped = blob[:1] + r.p256_pub(other_secret) + blob[1 + r.P256_POINT:]
    return [
        case("quick-unlock-se-seal", "ADR-0025",
             {"qk_public": x(qk_pub), "eph_secret": x(eph_secret), "auk": x(auk), "vault_id": x(VAULT_ID),
              "device_id": x(DEVICE_A), "nonce": x(nonce)},
             {"eph_public": x(eph_pub), "shared": x(shared), "wrap_key": x(wrap_key), "blob": x(blob)}),
        case("quick-unlock-se-open", "ADR-0025", base, {"auk": x(auk)}),
        reject("quick-unlock-se-other-device", "ADR-0025", {**base, "device_id": x(DEVICE_B)},
               "device id is in the AAD and the key derivation"),
        reject("quick-unlock-se-other-vault", "ADR-0025", {**base, "vault_id": x(fill("other-vault", 16))},
               "vault id is in the AAD and the key derivation"),
        reject("quick-unlock-se-wrong-qk", "ADR-0025", {**base, "qk_secret": x(other_secret)},
               "a different Secure Enclave key derives a different wrap key"),
        reject("quick-unlock-se-flipped-bit", "ADR-0025", {**base, "blob": x(bytes(flipped))},
               "any change to the ciphertext fails the tag"),
        reject("quick-unlock-se-swapped-ephemeral", "ADR-0025", {**base, "blob": x(swapped)},
               "another ephemeral public key gives another shared secret"),
        reject("quick-unlock-se-wrong-version", "ADR-0025", {**base, "blob": x(b"\x02" + blob[1:])},
               "only version 1 exists"),
        reject("quick-unlock-se-truncated", "ADR-0025", {**base, "blob": x(blob[:80])},
               "shorter than the smallest possible blob"),
        reject("quick-unlock-se-bad-point", "ADR-0025", {**base, "blob": x(blob[:1] + bytes(65) + blob[66:])},
               "the ephemeral key is not a point on the curve"),
    ]


def pairing():
    cases = []
    s = fill("invite-secret", 13)
    s = bytes([s[0] & 0x0F]) + s[1:]  # 100 bits
    code = r.invite_code_format(s)
    assert r.invite_secret_bytes(code) == s
    cases.append(case("invite-code-format", "pairing-recovery.md 3.3", {"s": x(s)}, {"code": code}))
    cases.append(case("invite-code-parse", "pairing-recovery.md 3.3", {"code": code.lower().replace("-", " ")},
                      {"s": x(s)}))
    et_priv, en_priv = fill("et-priv", 32), fill("en-priv", 32)
    et_pub, en_pub = r.x25519_pub(et_priv), r.x25519_pub(en_priv)
    k_n = r.invite_keys(s, VAULT_ID, en_priv, et_pub)
    k_t = r.invite_keys(s, VAULT_ID, et_priv, en_pub)
    assert k_n == k_t
    cases.append(case("invite-keys", "pairing-recovery.md 3.3",
                      {"s": x(s), "vault_id": x(VAULT_ID), "en_priv": x(en_priv), "et_priv": x(et_priv),
                       "et_pub": x(et_pub), "en_pub": x(en_pub)},
                      {"invite_id": x(k_n["invite_id"]), "k_req": x(k_n["k_req"]), "k_resp": x(k_n["k_resp"])},
                      note="both sides derive the same keys"))
    req_plain = cbor2.dumps({"device_id": DEVICE_B, "device_name": "Work MacBook", "sign_pub": b"\x01" * 32,
                             "agree_pub": b"\x02" * 32}, canonical=True)
    ia = r.aad("invite-req", VAULT_ID, k_n["invite_id"])
    nonce = fill("nonce-req", 24)
    req_file = en_pub + r.seal(k_n["k_req"], req_plain, ia, nonce)
    cases.append(case("invite-req", "pairing-recovery.md 3.3 step 3",
                      {"en_pub": x(en_pub), "k_req": x(k_n["k_req"]), "plaintext_cbor": x(req_plain),
                       "vault_id": x(VAULT_ID), "invite_id": x(k_n["invite_id"]), "nonce": x(nonce)},
                      {"req_file": x(req_file)}))
    cases.append(case("device-fingerprint", "pairing-recovery.md 3.3 step 4", {"sign_pub": x(b"\x01" * 32)},
                      {"fingerprint": r.device_fingerprint(b"\x01" * 32)}))
    resp_plain = cbor2.dumps({"secret_key": SK, "device_id": DEVICE_B}, canonical=True)
    ra = r.aad("invite-resp", VAULT_ID, k_n["invite_id"])
    rn = fill("nonce-resp", 24)
    resp = r.seal(k_n["k_resp"], resp_plain, ra, rn)
    cases.append(case("invite-resp", "pairing-recovery.md 3.3 step 5",
                      {"k_resp": x(k_n["k_resp"]), "plaintext_cbor": x(resp_plain), "vault_id": x(VAULT_ID),
                       "invite_id": x(k_n["invite_id"]), "nonce": x(rn)}, {"resp_file": x(resp)}))
    wrong_s = fill("wrong-secret", 13)
    wrong_s = bytes([wrong_s[0] & 0x0F]) + wrong_s[1:]
    k_bad = r.invite_keys(wrong_s, VAULT_ID, en_priv, et_pub)
    cases.append(reject("invite-resp-wrong-code", "pairing-recovery.md 3.3",
                        {"k_resp": x(k_bad["k_resp"]), "resp_file": x(resp), "vault_id": x(VAULT_ID),
                         "invite_id": x(k_n["invite_id"])}, "without s the keys differ: storage attacker cannot read"))
    swapped_et = r.x25519_pub(fill("attacker-et", 32))
    k_sw = r.invite_keys(s, VAULT_ID, en_priv, swapped_et)
    cases.append(reject("invite-resp-swapped-et-pub", "pairing-recovery.md 3.3",
                        {"k_resp": x(k_sw["k_resp"]), "resp_file": x(resp), "vault_id": x(VAULT_ID),
                         "invite_id": x(k_n["invite_id"])}, "a swapped Et_pub yields different keys"))
    for name, bad in (("too-short", code[:-5]), ("bad-char", code[:-1] + "U")):
        cases.append(reject(f"invite-code-{name}", "pairing-recovery.md 3.3", {"code": bad}, "invalid code"))
    return {"title": "Invite pairing", "cases": cases}


def seal_to():
    rk_priv = fill("rk-priv", 32)
    rk_pub = r.x25519_pub(rk_priv)
    eph = fill("eph-priv", 32)
    nonce = fill("nonce-seal-to", 24)
    a = r.aad("vk-wrap-rk", VAULT_ID, r.u32(1))
    blob = r.seal_to(rk_pub, VK, a, eph, nonce)
    base = {"rk_priv": x(rk_priv), "blob": x(blob), "vault_id": x(VAULT_ID), "key_epoch": 1}
    # rewrap after rotation needs only rk_pub
    vk2 = fill("vault-key-2", 32)
    blob2 = r.seal_to(rk_pub, vk2, r.aad("vk-wrap-rk", VAULT_ID, r.u32(2)), fill("eph-priv-2", 32),
                      fill("nonce-seal-to-2", 24))
    return {"title": "Recovery Key: seal_to and rewrap", "cases": [
        case("seal-to", "pairing-recovery.md 7.1",
             {"rk_pub": x(rk_pub), "plaintext": x(VK), "vault_id": x(VAULT_ID), "key_epoch": 1,
              "eph_priv": x(eph), "nonce": x(nonce)}, {"wrapped_vk_rk": x(blob)}),
        case("open-seal-to", "pairing-recovery.md 7.3", base, {"vk": x(VK)}),
        case("rewrap-after-rotation-needs-only-rk-pub", "pairing-recovery.md 7.1, crypto.md 9.1",
             {"rk_pub": x(rk_pub), "new_vk": x(vk2), "vault_id": x(VAULT_ID), "key_epoch": 2,
              "eph_priv": x(fill("eph-priv-2", 32)), "nonce": x(fill("nonce-seal-to-2", 24))},
             {"wrapped_vk_rk": x(blob2)}),
        reject("open-seal-to-wrong-rk", "pairing-recovery.md 7.3", {**base, "rk_priv": x(fill("wrong-rk", 32))},
               "wrong Recovery Key"),
        reject("open-seal-to-wrong-epoch", "pairing-recovery.md 7.1", {**base, "key_epoch": 2}, "epoch is in the AAD"),
        reject("open-seal-to-tampered", "pairing-recovery.md 7.1", {**base, "blob": x(flip(blob))}, "tag"),
    ]}


def hlc():
    cases = []

    def run(name, start, steps, spec="sync.md 5"):
        h = r.Hlc(*start)
        out = []
        for st in steps:
            if st[0] == "local":
                out.append(list(h.local(st[1])))
            else:
                out.append(list(h.receive(st[1], st[2], st[3])))
        cases.append(case(name, spec, {"start": list(start), "steps": [list(s) for s in steps]}, out))

    run("local-time-advances", (0, 0), [("local", 100), ("local", 105)])
    run("local-same-millisecond-increments-counter", (0, 0), [("local", 100), ("local", 100), ("local", 100)])
    run("local-clock-went-backwards", (200, 5), [("local", 150)])
    run("receive-remote-ahead", (100, 0), [("recv", 500, 7, 120)])
    run("receive-same-wall-both", (500, 3), [("recv", 500, 7, 400)])
    run("receive-local-ahead", (500, 3), [("recv", 400, 9, 450)])
    run("receive-now-ahead-of-both", (100, 4), [("recv", 90, 2, 700)])
    for_overflow = r.Hlc(100, 0xFFFF)
    try:
        for_overflow.local(100)
        raise AssertionError("expected overflow")
    except r.Reject:
        pass
    cases.append(reject("counter-overflow", "sync.md 5", {"start": [100, 0xFFFF], "steps": [["local", 100]]},
                        "counter would exceed 0xFFFF: wait one millisecond and retry"))
    now = 1_700_000_000_000
    cases.append(case("drift-hold", "sync.md 5",
                      {"now": now, "op_wall_ms": [now + r.MAX_DRIFT_MS, now + r.MAX_DRIFT_MS + 1]},
                      {"held": [r.is_held(now + r.MAX_DRIFT_MS, now), r.is_held(now + r.MAX_DRIFT_MS + 1, now)]}))
    return {"title": "Hybrid logical clock", "cases": cases}


def op(item, field, wall, counter, device, value):
    return {"item": item, "field": field, "wall": wall, "counter": counter, "device": device, "value": value}


A, B = DEVICE_A.hex(), DEVICE_B.hex()
I1, I2 = "item-1", "item-2"


def merge_cases():
    scenarios = {
        "same-field-later-wins": [op(I1, "title", 100, 0, A, "old"), op(I1, "title", 200, 0, B, "new")],
        "same-field-counter-breaks-tie": [op(I1, "title", 100, 1, A, "a"), op(I1, "title", 100, 2, B, "b")],
        "same-stamp-device-id-breaks-tie": [op(I1, "title", 100, 0, A, "a"), op(I1, "title", 100, 0, B, "b")],
        "different-fields-both-kept": [op(I1, "title", 100, 0, A, "t"), op(I1, "username", 100, 0, B, "u")],
        "delete-beats-concurrent-edit": [op(I1, "_deleted", 300, 0, A, False), op(I1, "_deleted", 400, 0, A, True),
                                         op(I1, "title", 500, 0, B, "edited after delete")],
        "restore-after-delete": [op(I1, "_deleted", 400, 0, A, True), op(I1, "_deleted", 500, 0, B, False)],
        "delete-vs-restore-same-wall-device-breaks-tie": [op(I1, "_deleted", 400, 0, A, True),
                                                           op(I1, "_deleted", 400, 0, B, False)],
        "two-items-independent": [op(I1, "title", 100, 0, A, "one"), op(I2, "title", 100, 0, B, "two")],
        "duplicate-op-idempotent": [op(I1, "title", 100, 0, A, "t"), op(I1, "title", 100, 0, A, "t")],
    }
    cases = []
    rng = random.Random(20)
    for name, ops in scenarios.items():
        expected = r.merge(ops)
        # every permutation (up to 6 elements) must give the same result
        for perm in itertools.permutations(ops):
            assert r.merge(list(perm)) == expected, name
        cases.append(case(name, "sync.md 6.2-6.3", {"ops": ops}, expected,
                          note="every ordering and duplication of these ops gives the same state"))
    # snapshot equivalence: state(snapshot) + remaining ops == replay of everything
    allops = [op(I1, "title", 100, 0, A, "a"), op(I1, "username", 110, 0, A, "u"), op(I1, "title", 150, 0, B, "b"),
              op(I2, "title", 120, 0, B, "x"), op(I1, "_deleted", 160, 0, A, True)]
    full = r.merge(allops)
    prefix = r.merge(allops[:3])
    as_ops = [op(i, f, v[0], v[1], v[2], v[3]) for i, fs in prefix.items() for f, v in fs.items()]
    assert r.merge(as_ops + allops[3:]) == full
    assert r.merge(as_ops + allops) == full  # applying ops already in the snapshot changes nothing
    cases.append(case("snapshot-plus-remaining-equals-replay", "sync.md 7",
                      {"snapshot_from_first_ops": 3, "ops": allops}, full))
    # purge horizon drops a ghost op for a purged item
    ghost = [op("purged-item", "title", 50, 0, A, "ghost"), op(I1, "title", 100, 0, A, "kept")]
    exp = r.merge(ghost, purge_horizon=(80, 0), known_items={I1})
    assert "purged-item" not in exp
    cases.append(case("purge-horizon-drops-ghost-op", "sync.md 6.4",
                      {"ops": ghost, "purge_horizon": [80, 0], "known_items": [I1]}, exp))
    ghost_new = [op("new-item", "title", 90, 0, A, "newer than horizon")]
    cases.append(case("op-newer-than-purge-horizon-kept", "sync.md 6.4",
                      {"ops": ghost_new, "purge_horizon": [80, 0], "known_items": []},
                      r.merge(ghost_new, purge_horizon=(80, 0), known_items=set())))
    return {"title": "Field-level merge", "cases": cases}


def sync_edge_cases():
    """Checks that need the reader state machine, not just a pure function. The reference only states the
    expected outcome; the Rust implementation must produce it."""
    cases = [
        {"name": "gap-blocks-later-files", "spec": "sync.md 8",
         "inputs": {"applied": {A: 3}, "present_files": {A: [5, 6]}}, "expected": {"applied": {A: 3},
                                                                                "waiting_for": {A: 4}}},
        {"name": "files-covered-by-snapshot-are-not-a-gap", "spec": "sync.md 7, 8",
         "inputs": {"applied": {A: 3}, "snapshot_hw": {A: 5}, "present_files": {A: [6]}},
         "expected": {"applied": {A: 6}}},
        {"name": "fork-same-seq-two-files", "spec": "sync.md 8",
         "inputs": {"applied": {A: 3}, "files": {A: {"4": ["file-x", "file-y"]}}},
         "expected": {"applied": {A: 3}, "device_stopped": A, "warning": "fork"}},
        {"name": "prev-hash-mismatch", "spec": "sync.md 8", "inputs": {"applied": {A: 3}, "file_4_prev_matches": False},
         "expected": {"applied": {A: 3}, "device_stopped": A, "warning": "fork"}},
        {"name": "rollback-shorter-listing", "spec": "sync.md 8",
         "inputs": {"applied": {A: 7}, "listing": {A: [1, 2, 3]}},
         "expected": {"applied": {A: 7}, "warning": "rollback", "never_lower_local_state": True}},
        {"name": "vault-json-lower-revision-ignored", "spec": "sync.md 3, 8",
         "inputs": {"accepted_revision": 5, "seen_revision": 4}, "expected": {"accepted_revision": 5, "warning": "rollback"}},
        {"name": "revoked-device-before-revoked-at-accepted", "spec": "sync.md 9",
         "inputs": {"revoked_at": [1000, 0], "op_stamp": [999, 9], "file_epoch": 1, "rotation_epoch": 2},
         "expected": "accept"},
        {"name": "revoked-device-after-revoked-at-ignored", "spec": "sync.md 9",
         "inputs": {"revoked_at": [1000, 0], "op_stamp": [1000, 1], "file_epoch": 1, "rotation_epoch": 2},
         "expected": "ignore"},
        {"name": "revoked-device-new-epoch-ignored", "spec": "sync.md 9",
         "inputs": {"revoked_at": [1000, 0], "op_stamp": [900, 0], "file_epoch": 2, "rotation_epoch": 2},
         "expected": "ignore"},
    ]
    return {"title": "Reader state machine outcomes", "cases": cases}


FILES = {
    "secret-key.json": secret_key, "recovery-key.json": recovery_key, "kdf.json": kdf, "key-hierarchy.json": key_hierarchy, "items.json": items,
    "files.json": files, "signatures.json": signatures, "rotation.json": rotation, "quick-unlock.json": quick_unlock,
    "pairing.json": pairing, "seal-to.json": seal_to, "hlc.json": hlc, "merge.json": merge_cases,
    "sync-reader.json": sync_edge_cases, "audit.json": audit,
}


def render(doc: dict) -> str:
    doc = {"format": "figlock-vectors-1", "generator": "docs/spec/vectors/generate.py", **doc}
    return json.dumps(doc, indent=2, sort_keys=False) + "\n"


# --------------------------------- verification of the committed vectors -----------------------------------
def b(h):
    return bytes.fromhex(h) if h is not None else None


def verify_case(fname: str, c: dict) -> None:
    """Independently recompute the case from its inputs with the reference and compare."""
    i, e = c["inputs"], c["expected"]
    name = c["name"]

    def expect_reject(fn):
        try:
            fn()
        except r.Reject:
            return
        raise AssertionError(f"{fname}:{name} should be rejected")

    if fname == "secret-key.json":
        if e == "reject":
            expect_reject(lambda: r.secret_key_parse(i["text"]))
        elif "text" in e:
            assert r.secret_key_format(b(i["sk"])) == e["text"]
        else:
            assert r.secret_key_parse(i["text"]) == b(e["sk"])
    elif fname == "recovery-key.json":
        if e == "reject":
            expect_reject(lambda: r.recovery_key_parse(i["text"]))
        elif "text" in e:
            assert r.recovery_key_format(b(i["rk_priv"])) == e["text"]
        else:
            assert r.recovery_key_parse(i["text"]) == b(e["rk_priv"])
    elif fname == "kdf.json":
        if e == "reject":
            expect_reject(lambda: r.check_kdf_params(i["m_kib"], i["t"], i["p"]))
        elif "passwords_equal_after_nfkd" in i:
            for pw in i["passwords_equal_after_nfkd"]:
                assert r.pw_key(pw, b(i["salt"]), i["m_kib"], i["t"], i["p"]) == b(e["pw_key"])
        else:
            assert r.pw_key(i["password"], b(i["salt"]), i["m_kib"], i["t"], i["p"]) == b(e["pw_key"])
    elif fname == "key-hierarchy.json":
        if "kdf_params_bytes" in i and "wrapped_vk" in i:
            if e == "reject":
                expect_reject(lambda: r.unwrap_vk(b(i["auk"]), b(i["wrapped_vk"]), b(i["vault_id"]), i["key_epoch"],
                                                  b(i["kdf_params_bytes"])))
            else:
                assert r.unwrap_vk(b(i["auk"]), b(i["wrapped_vk"]), b(i["vault_id"]), i["key_epoch"],
                                   b(i["kdf_params_bytes"])) == b(e["vk"])
        elif "nonce" in i:
            assert r.wrap_vk(b(i["auk"]), b(i["vk"]), b(i["vault_id"]), i["key_epoch"], b(i["kdf_params_bytes"]),
                             b(i["nonce"])) == b(e["wrapped_vk"])
        elif "vk" in i:
            d = r.derive_from_vk(b(i["vk"]))
            for k in ("lk", "wk", "authority_seed", "authority_pub"):
                assert d[k] == b(e[k])
        else:
            a = r.auk(i["password"], b(i["sk"]), b(i["vault_id"]), b(i["salt"]), i["m_kib"], i["t"], i["p"])
            assert a == b(e["auk"])
            if "pw_key" in e:
                assert r.pw_key(i["password"], b(i["salt"]), i["m_kib"], i["t"], i["p"]) == b(e["pw_key"])
                assert r.sk_key(b(i["sk"]), b(i["vault_id"])) == b(e["sk_key"])
    elif fname == "items.json":
        if "wrapped_ik" in i:
            expect_reject(lambda: r.unwrap_ik(b(i["wk"]), b(i["wrapped_ik"]), b(i["vault_id"]), b(i["item_id"]),
                                              i["key_epoch"]))
        elif "ik" in i and "wk" in i:
            assert r.wrap_ik(b(i["wk"]), b(i["ik"]), b(i["vault_id"]), b(i["item_id"]), i["key_epoch"],
                             b(i["nonce"])) == b(e["wrapped_ik"])
        elif "nonce" in i:
            assert r.field_blob(b(i["ik"]), b(i["value_cbor"]), b(i["vault_id"]), b(i["item_id"]), i["field_id"],
                                b(i["hlc"]), b(i["nonce"])) == b(e["field_blob"])
        elif e == "reject":
            expect_reject(lambda: r.open_field(b(i["ik"]), b(i["field_blob"]), b(i["vault_id"]), b(i["item_id"]),
                                               i["field_id"], b(i["hlc"])))
        else:
            assert r.open_field(b(i["ik"]), b(i["field_blob"]), b(i["vault_id"]), b(i["item_id"]), i["field_id"],
                                b(i["hlc"])) == b(e["value_cbor"])
    elif fname == "files.json":
        kind = r.KIND_OP if i["kind"] == "op" else r.KIND_SNAP
        if "nonce" in i:
            assert r.build_file(kind, b(i["lk"]), b(i["device_sign_seed"]), b(i["vault_id"]), b(i["device_id"]),
                                i["seq"], i["key_epoch"], b(i["payload_cbor"]), b(i["nonce"])) == b(e["file"])
        elif e == "reject":
            expect_reject(lambda: r.read_file(b(i["file"]), b(i["lk"]), b(i["device_sign_pub"]), b(i["vault_id"]),
                                              b(i["device_id"]), i["seq"], kind))
        else:
            assert r.read_file(b(i["file"]), b(i["lk"]), b(i["device_sign_pub"]), b(i["vault_id"]),
                               b(i["device_id"]), i["seq"], kind) == b(e["payload_cbor"])
    elif fname == "audit.json":
        kinds = {"audit": r.KIND_AUDIT, "op": r.KIND_OP}
        if "vk" in i:
            assert r.derive_from_vk(b(i["vk"]))["ak"] == b(e["ak"])
        elif "ev" in i:
            assert r.audit_payload(b(i["prev"]), i["t"], i["ev"], i["d"]).decode() == e["payload_jcs"]
        elif "nonce" in i:
            assert r.build_file(r.KIND_AUDIT, b(i["ak"]), b(i["device_sign_seed"]), b(i["vault_id"]),
                                b(i["device_id"]), i["seq"], i["key_epoch"], i["payload_jcs"].encode(),
                                b(i["nonce"])) == b(e["file"])
        elif "files" in i:
            args = ([b(f) for f in i["files"]], b(i["ak"]), b(i["device_sign_pub"]), b(i["vault_id"]),
                    b(i["device_id"]))
            if e == "reject":
                expect_reject(lambda: r.read_audit_chain(*args))
            else:
                out = r.read_audit_chain(*args)
                assert len(out) == e["entries"]
                files_ = [b(f) for f in i["files"]]
                assert (r.sha256(files_[-1]) if files_ else r.AUDIT_ZERO) == b(e["head"])
        else:
            args = (b(i["file"]), b(i["ak"]), b(i["device_sign_pub"]), b(i["vault_id"]), b(i["device_id"]),
                    i["seq"], kinds[i["kind"]])
            if e == "reject":
                expect_reject(lambda: r.read_file(*args))
            else:
                assert r.read_file(*args).decode() == e["payload_jcs"]
    elif fname == "signatures.json":
        if "authority_seed" in i:
            obj = i.get("record", i.get("vault_json"))
            ctx = "device-record" if "record" in i else "vault-json"
            assert r.jcs(obj).decode() == i["jcs"]
            assert r.sign_json(b(i["authority_seed"]), ctx, obj) == b(e["signature"])
        else:
            obj = i.get("record", i.get("vault_json"))
            ctx = i.get("context", "device-record" if "record" in i else "vault-json")
            if e == "reject":
                expect_reject(lambda: r.verify_json(b(i["authority_pub"]), ctx, obj, b(i["signature"])))
            else:
                r.verify_json(b(i["authority_pub"]), ctx, obj, b(i["signature"]))
    elif fname == "rotation.json":
        if "nonce" in i:
            assert r.epoch_key(b(i["new_vk"]), b(i["old_vk"]), b(i["vault_id"]), i["old_epoch"], i["new_epoch"],
                               b(i["nonce"])) == b(e["epoch_key"])
        elif "epoch_key" in i:
            a = r.aad("epoch-key", b(i["vault_id"]), r.u32(i["old_epoch"]), r.u32(i["new_epoch"]))
            if e == "reject":
                expect_reject(lambda: r.open_(b(i["new_vk"]), b(i["epoch_key"]), a))
            else:
                assert r.open_(b(i["new_vk"]), b(i["epoch_key"]), a) == b(e["old_vk"])
        else:
            o, n = r.derive_from_vk(b(i["old_vk"])), r.derive_from_vk(b(i["new_vk"]))
            assert (o["lk"], n["lk"], o["authority_pub"], n["authority_pub"]) == (
                b(e["old_lk"]), b(e["new_lk"]), b(e["old_authority_pub"]), b(e["new_authority_pub"]))
    elif fname == "quick-unlock.json":
        if name.startswith("quick-unlock-se-"):
            if name == "quick-unlock-se-seal":
                blob = r.quick_unlock_se_blob(b(i["qk_public"]), b(i["eph_secret"]), b(i["auk"]), b(i["vault_id"]),
                                              b(i["device_id"]), b(i["nonce"]))
                assert blob == b(e["blob"])
                assert r.p256_pub(b(i["eph_secret"])) == b(e["eph_public"])
                assert r.p256_agree(b(i["eph_secret"]), b(i["qk_public"])) == b(e["shared"])
            elif e == "reject":
                expect_reject(lambda: r.quick_unlock_se_open(b(i["qk_secret"]), b(i["blob"]), b(i["vault_id"]),
                                                             b(i["device_id"])))
            else:
                assert r.quick_unlock_se_open(b(i["qk_secret"]), b(i["blob"]), b(i["vault_id"]),
                                              b(i["device_id"])) == b(e["auk"])
        elif "nonce" in i:
            assert r.quick_unlock_blob(b(i["qk"]), b(i["auk"]), b(i["vault_id"]), b(i["device_id"]),
                                       b(i["nonce"])) == b(e["blob"])
        elif e == "reject":
            expect_reject(lambda: r.open_(b(i["qk"]), b(i["blob"]), r.aad("quick-unlock", b(i["vault_id"]),
                                                                            b(i["device_id"]))))
        else:
            assert r.open_(b(i["qk"]), b(i["blob"]), r.aad("quick-unlock", b(i["vault_id"]),
                                                            b(i["device_id"]))) == b(e["auk"])
    elif fname == "pairing.json":
        if name == "invite-code-format":
            assert r.invite_code_format(b(i["s"])) == e["code"]
        elif name == "invite-code-parse":
            assert r.invite_secret_bytes(i["code"]) == b(e["s"])
        elif name.startswith("invite-code-"):
            expect_reject(lambda: r.invite_secret_bytes(i["code"]))
        elif name == "invite-keys":
            k1 = r.invite_keys(b(i["s"]), b(i["vault_id"]), b(i["en_priv"]), b(i["et_pub"]))
            k2 = r.invite_keys(b(i["s"]), b(i["vault_id"]), b(i["et_priv"]), b(i["en_pub"]))
            assert k1 == k2
            assert (k1["invite_id"], k1["k_req"], k1["k_resp"]) == (b(e["invite_id"]), b(e["k_req"]), b(e["k_resp"]))
        elif name == "device-fingerprint":
            assert r.device_fingerprint(b(i["sign_pub"])) == e["fingerprint"]
        elif name == "invite-req":
            a = r.aad("invite-req", b(i["vault_id"]), b(i["invite_id"]))
            assert b(i["en_pub"]) + r.seal(b(i["k_req"]), b(i["plaintext_cbor"]), a, b(i["nonce"])) == b(e["req_file"])
        elif name == "invite-resp":
            a = r.aad("invite-resp", b(i["vault_id"]), b(i["invite_id"]))
            assert r.seal(b(i["k_resp"]), b(i["plaintext_cbor"]), a, b(i["nonce"])) == b(e["resp_file"])
        else:
            a = r.aad("invite-resp", b(i["vault_id"]), b(i["invite_id"]))
            expect_reject(lambda: r.open_(b(i["k_resp"]), b(i["resp_file"]), a))
    elif fname == "seal-to.json":
        if "eph_priv" in i:
            vk = b(i.get("plaintext") or i["new_vk"])
            a = r.aad("vk-wrap-rk", b(i["vault_id"]), r.u32(i["key_epoch"]))
            assert r.seal_to(b(i["rk_pub"]), vk, a, b(i["eph_priv"]), b(i["nonce"])) == b(e["wrapped_vk_rk"])
        else:
            a = r.aad("vk-wrap-rk", b(i["vault_id"]), r.u32(i["key_epoch"]))
            if e == "reject":
                expect_reject(lambda: r.open_seal_to(b(i["rk_priv"]), b(i["blob"]), a))
            else:
                assert r.open_seal_to(b(i["rk_priv"]), b(i["blob"]), a) == b(e["vk"])
    elif fname == "hlc.json":
        if name == "counter-overflow":
            expect_reject(lambda: r.Hlc(*i["start"]).local(i["steps"][0][1]))
        elif name == "drift-hold":
            assert [r.is_held(w, i["now"]) for w in i["op_wall_ms"]] == e["held"]
        else:
            h, out = r.Hlc(*i["start"]), []
            for st in i["steps"]:
                out.append(list(h.local(st[1]) if st[0] == "local" else h.receive(st[1], st[2], st[3])))
            assert out == e
    elif fname == "merge.json":
        ops = i["ops"]
        ph = tuple(i["purge_horizon"]) if "purge_horizon" in i else None
        known = set(i["known_items"]) if "known_items" in i else None
        assert r.merge(ops, ph, known) == e
        if ph is None and "snapshot_from_first_ops" not in i:
            for _ in range(20):
                shuffled = ops + ops[: len(ops) // 2]  # with duplicates
                random.Random(len(shuffled)).shuffle(shuffled)
                assert r.merge(shuffled) == e
    elif fname == "sync-reader.json":
        pass  # outcomes for the Rust reader state machine; nothing in the reference to recompute
    else:
        raise AssertionError(f"no verifier for {fname}")


def check_primitives() -> None:
    """Published vectors for the primitives, so a wiring mistake in reference.py cannot hide behind the
    vectors it generates. Argon2id is covered by the argon2 library's own RFC 9106 tests; the wrapper only
    fixes type=Id, version 0x13 and an empty secret and associated data."""
    # RFC 5869 A.3: SHA-256, zero-length salt and info
    okm = r.hkdf(bytes.fromhex("0b" * 22), "", 42)
    assert okm.hex() == ("8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d"
                         "9d201395faa4b61a96c8"), "hkdf"
    # RFC 8032 7.1 test 1 (empty message)
    seed = bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
    sk = r.SigningKey(seed)
    assert bytes(sk.verify_key).hex() == "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a", "ed25519 pub"
    assert sk.sign(b"").signature.hex() == ("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155"
                                            "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"), "ed25519 sig"
    # RFC 7748 6.1
    a = bytes.fromhex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a")
    bp = bytes.fromhex("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f")
    assert r.x25519_pub(a).hex() == "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a", "x25519 pub"
    assert r.x25519(a, bp).hex() == "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742", "x25519 shared"
    # draft-irtf-cfrg-xchacha-03 A.3.1
    key = bytes(range(0x80, 0xA0))
    nonce = bytes(range(0x40, 0x58))
    ad = bytes.fromhex("50515253c0c1c2c3c4c5c6c7")
    pt = (b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, "
          b"sunscreen would be it.")
    env = r.seal(key, pt, ad, nonce)
    assert env[25:25 + 8].hex() == "bd6d179d3e83d43b", "xchacha ct"
    assert env[-16:].hex() == "c0875924c1c7987947deafd8780acf49", "xchacha tag"
    assert r.open_(key, env, ad) == pt


def slip39():
    import shamir_mnemonic as sm
    path = HERE / "slip39.json"
    rk_priv = fill("rk-priv", 32)
    if path.exists():
        return json.loads(path.read_text())
    cases = []
    for name, (m, n) in (("2-of-3", (2, 3)), ("3-of-5", (3, 5))):
        groups = sm.generate_mnemonics(1, [(m, n)], rk_priv, b"")
        shares = groups[0]
        cases.append({"name": name, "spec": "pairing-recovery.md 7.2",
                      "inputs": {"master_secret": x(rk_priv), "threshold": m, "share_count": n, "passphrase": ""},
                      "shares": shares, "expected": x(rk_priv)})
    return {"format": "figlock-vectors-1", "generator": "docs/spec/vectors/generate.py",
            "title": "SLIP-0039 shares of the Recovery Key (generated once; verify only)",
            "note": "Official SLIP-0039 vectors are run by the Rust implementation in addition to these.",
            "cases": cases}


def verify_slip39(doc: dict) -> None:
    import shamir_mnemonic as sm
    for c in doc["cases"]:
        m, n = c["inputs"]["threshold"], c["inputs"]["share_count"]
        shares = c["shares"]
        assert len(shares) == n and all(len(s.split()) == 33 for s in shares)
        for combo in itertools.combinations(shares, m):
            assert sm.combine_mnemonics(list(combo), b"").hex() == c["expected"]
        # below threshold must fail
        try:
            sm.combine_mnemonics(shares[: m - 1], b"")
            raise AssertionError("below-threshold recovery must fail")
        except sm.MnemonicError:
            pass
        # a corrupted word must fail the built-in checksum
        words = shares[0].split()
        words[3] = "academic" if words[3] != "academic" else "acid"
        try:
            sm.combine_mnemonics([" ".join(words)] + shares[1:m], b"")
            raise AssertionError("corrupted share must fail")
        except sm.MnemonicError:
            pass


def main() -> int:
    check = "--check" in sys.argv
    rendered = {name: render(fn()) for name, fn in FILES.items()}
    slip = slip39()
    rendered["slip39.json"] = json.dumps(slip, indent=2) + "\n"
    failures = 0
    for name, text in rendered.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text() != text:
                print(f"DRIFT {name}: committed file differs from regenerated output")
                failures += 1
        else:
            path.write_text(text)
            print(f"wrote {name}")
    if check:
        check_primitives()
        print("primitives match RFC 5869, 8032, 7748 and the XChaCha draft")
        for name in FILES:
            doc = json.loads((HERE / name).read_text())
            for c in doc["cases"]:
                try:
                    verify_case(name, c)
                except Exception as e:  # noqa: BLE001
                    print(f"FAIL {name}: {c['name']}: {type(e).__name__}: {e}")
                    failures += 1
        verify_slip39(json.loads((HERE / "slip39.json").read_text()))
        total = sum(len(json.loads((HERE / n).read_text())["cases"]) for n in [*FILES, "slip39.json"])
        print(f"{total} cases checked, {failures} failures")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
