"""Reference implementation of the FigLock specs, used only to produce and check test vectors (FIG-20).

It follows docs/spec/crypto.md, sync.md and pairing-recovery.md literally and is deliberately independent
of the Rust core, so that the Rust implementation (FIG-24, FIG-26) is checked against a second one.
Not for production use: no zeroization, no constant-time guarantees beyond the libraries.
"""
from __future__ import annotations

import hashlib
import json
import struct
import unicodedata

from argon2.low_level import Type, hash_secret_raw
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey, X25519PublicKey
from cryptography.hazmat.primitives.kdf.hkdf import HKDF
from nacl import bindings as nb
from nacl.signing import SigningKey, VerifyKey
from nacl.exceptions import BadSignatureError

VERSION = b"\x01"
CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"


class Reject(Exception):
    """Any verification or decryption failure. Callers get no detail, as required by crypto.md 10."""


# ---- encoding helpers (crypto.md 3) ----
def u16(n: int) -> bytes:
    return struct.pack(">H", n)


def u32(n: int) -> bytes:
    return struct.pack(">I", n)


def u64(n: int) -> bytes:
    return struct.pack(">Q", n)


def lp(x: bytes) -> bytes:
    return u32(len(x)) + x


def aad(ctx: str, *parts: bytes) -> bytes:
    c = ctx.encode()
    return u16(len(c)) + c + b"".join(lp(p) for p in parts)


def sha256(*parts: bytes) -> bytes:
    h = hashlib.sha256()
    for p in parts:
        h.update(p)
    return h.digest()


def hkdf(ikm: bytes, info: str, length: int = 32, salt: bytes | None = None) -> bytes:
    return HKDF(algorithm=hashes.SHA256(), length=length, salt=salt, info=info.encode()).derive(ikm)


def jcs(obj) -> bytes:
    """RFC 8785 for the value types we use (ASCII keys, strings, integers, booleans, lists, maps)."""
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


# ---- AEAD (crypto.md 3) ----
def seal(key: bytes, plaintext: bytes, associated: bytes, nonce: bytes) -> bytes:
    assert len(key) == 32 and len(nonce) == 24
    ct = nb.crypto_aead_xchacha20poly1305_ietf_encrypt(plaintext, associated, nonce, key)
    return VERSION + nonce + ct


def open_(key: bytes, envelope: bytes, associated: bytes) -> bytes:
    if len(envelope) < 1 + 24 + 16 or envelope[:1] != VERSION:
        raise Reject()
    try:
        return nb.crypto_aead_xchacha20poly1305_ietf_decrypt(envelope[25:], associated, envelope[1:25], key)
    except Exception as e:  # noqa: BLE001 - library raises a generic error; we normalise it
        raise Reject() from e


# ---- Secret Key (crypto.md 4.1) ----
def b32_encode(data: bytes) -> str:
    bits = "".join(f"{b:08b}" for b in data)
    assert len(bits) % 5 == 0
    return "".join(CROCKFORD[int(bits[i:i + 5], 2)] for i in range(0, len(bits), 5))


def device_fingerprint(sign_pub: bytes) -> str:
    """pairing-recovery.md 3.3 step 4: 80 bits of SHA-256, Crockford, four groups of four."""
    t = b32_encode(sha256(b"figlock/v1/device-fingerprint", b"\x00", sign_pub)[:10])
    return "-".join(t[i:i + 4] for i in range(0, 16, 4))


def b32_decode(text: str) -> bytes:
    bits = "".join(f"{CROCKFORD.index(c):05b}" for c in text)
    assert len(bits) % 8 == 0
    return bytes(int(bits[i:i + 8], 2) for i in range(0, len(bits), 8))


def secret_key_format(sk: bytes) -> str:
    assert len(sk) == 16
    check = sha256(b"figlock/v1/sk-check", sk)[:4]
    s = b32_encode(sk + check)
    return "FL1-" + "-".join(s[i:i + 4] for i in range(0, 32, 4))


def secret_key_parse(text: str) -> bytes:
    t = text.upper().replace("-", "").replace(" ", "")
    if not t.startswith("FL1"):
        raise Reject()
    t = t[3:].replace("I", "1").replace("L", "1").replace("O", "0")
    if len(t) != 32 or any(c not in CROCKFORD for c in t):
        raise Reject()
    raw = b32_decode(t)
    sk, check = raw[:16], raw[16:]
    if sha256(b"figlock/v1/sk-check", sk)[:4] != check:
        raise Reject()
    return sk


# ---- key derivation (crypto.md 5, 6) ----
def kdf_params_bytes(m: int, t: int, p: int, salt: bytes) -> bytes:
    return u32(m) + u32(t) + u32(p) + lp(salt)


FLOOR = (32768, 2, 1)
CEILING = (1048576, 10, 16)


def check_kdf_params(m: int, t: int, p: int) -> None:
    if m < FLOOR[0] or t < FLOOR[1] or p < FLOOR[2] or m > CEILING[0] or t > CEILING[1] or p > CEILING[2]:
        raise Reject()


def pw_key(password: str, salt: bytes, m: int, t: int, p: int) -> bytes:
    check_kdf_params(m, t, p)
    pw = unicodedata.normalize("NFKD", password).encode()
    return hash_secret_raw(pw, salt, time_cost=t, memory_cost=m, parallelism=p, hash_len=32,
                           type=Type.ID, version=19)


def sk_key(sk: bytes, vault_id: bytes) -> bytes:
    return hkdf(sk, "figlock/v1/secret-key", salt=vault_id)


def auk(password: str, sk: bytes, vault_id: bytes, salt: bytes, m: int, t: int, p: int) -> bytes:
    return hkdf(pw_key(password, salt, m, t, p) + sk_key(sk, vault_id), "figlock/v1/account-unlock-key",
                salt=vault_id)


def derive_from_vk(vk: bytes) -> dict:
    seed = hkdf(vk, "figlock/v1/authority-seed")
    sig = SigningKey(seed)
    return {
        "lk": hkdf(vk, "figlock/v1/log-key"),
        "wk": hkdf(vk, "figlock/v1/item-key-wrap"),
        "authority_seed": seed,
        "authority_pub": bytes(sig.verify_key),
        "ak": hkdf(vk, "figlock/v1/audit-key"),
    }


def wrap_vk(auk_: bytes, vk: bytes, vault_id: bytes, epoch: int, params: bytes, nonce: bytes) -> bytes:
    return seal(auk_, vk, aad("vk-wrap", vault_id, u32(epoch), params), nonce)


def unwrap_vk(auk_: bytes, wrapped: bytes, vault_id: bytes, epoch: int, params: bytes) -> bytes:
    return open_(auk_, wrapped, aad("vk-wrap", vault_id, u32(epoch), params))


# ---- items (crypto.md 7.1) ----
def hlc_bytes(wall_ms: int, counter: int) -> bytes:
    return wall_ms.to_bytes(6, "big") + u16(counter)


def wrap_ik(wk: bytes, ik: bytes, vault_id: bytes, item_id: bytes, epoch: int, nonce: bytes) -> bytes:
    return seal(wk, ik, aad("item-key", vault_id, item_id, u32(epoch)), nonce)


def unwrap_ik(wk: bytes, wrapped: bytes, vault_id: bytes, item_id: bytes, epoch: int) -> bytes:
    return open_(wk, wrapped, aad("item-key", vault_id, item_id, u32(epoch)))


def field_blob(ik: bytes, value_cbor: bytes, vault_id: bytes, item_id: bytes, field_id: str, hlc: bytes,
               nonce: bytes) -> bytes:
    return seal(ik, value_cbor, aad("field", vault_id, item_id, field_id.encode(), hlc), nonce)


def open_field(ik: bytes, blob: bytes, vault_id: bytes, item_id: bytes, field_id: str, hlc: bytes) -> bytes:
    return open_(ik, blob, aad("field", vault_id, item_id, field_id.encode(), hlc))


# ---- files: container, signature (crypto.md 7.2-7.3, sync.md 4) ----
KIND_OP, KIND_SNAP, KIND_AUDIT = 1, 2, 3


def file_header(kind: int, epoch: int) -> bytes:
    return b"FL" + b"\x01" + bytes([kind]) + u32(epoch)


def file_aad(kind: int, vault_id: bytes, device_id: bytes, seq: int, epoch: int) -> bytes:
    ctx = {KIND_OP: "log", KIND_SNAP: "snap", KIND_AUDIT: "audit"}[kind]
    return aad(ctx, vault_id, device_id, u64(seq), u32(epoch))


def file_sig_input(vault_id: bytes, device_id: bytes, seq: int, header: bytes, envelope: bytes) -> bytes:
    return (b"figlock/v1/file-sig" + b"\x00" + vault_id + device_id + u64(seq) + sha256(header, envelope))


def build_file(kind: int, lk: bytes, sign_seed: bytes, vault_id: bytes, device_id: bytes, seq: int, epoch: int,
               payload_cbor: bytes, nonce: bytes) -> bytes:
    header = file_header(kind, epoch)
    envelope = seal(lk, payload_cbor, file_aad(kind, vault_id, device_id, seq, epoch), nonce)
    sig = SigningKey(sign_seed).sign(file_sig_input(vault_id, device_id, seq, header, envelope)).signature
    return header + u32(len(envelope)) + envelope + sig


def read_file(data: bytes, lk: bytes, sign_pub: bytes | None, vault_id: bytes, device_id: bytes, seq: int,
              expect_kind: int) -> bytes:
    """Verify then decrypt. `sign_pub` None means the device has no valid, non-revoked record."""
    if sign_pub is None or len(data) < 8 + 4 + 64:
        raise Reject()
    header = data[:8]
    if header[:3] != b"FL\x01" or header[3] != expect_kind:
        raise Reject()
    epoch = struct.unpack(">I", header[4:8])[0]
    (n,) = struct.unpack(">I", data[8:12])
    if len(data) != 12 + n + 64:
        raise Reject()
    envelope, sig = data[12:12 + n], data[12 + n:]
    try:
        VerifyKey(sign_pub).verify(file_sig_input(vault_id, device_id, seq, header, envelope), sig)
    except BadSignatureError as e:
        raise Reject() from e
    return open_(lk, envelope, file_aad(expect_kind, vault_id, device_id, seq, epoch))


# ---- audit records (crypto.md 7.5, broker.md 5) ----
AUDIT_ZERO = b"\x00" * 32


def audit_payload(prev: bytes, t: int, ev: str, d: dict) -> bytes:
    return jcs({"v": 1, "prev": prev.hex(), "t": t, "ev": ev, "d": d})


def read_audit_chain(files: list, ak: bytes, sign_pub: bytes, vault_id: bytes, device_id: bytes) -> list:
    """Verify every record (signature, then decryption), seq 1..n, canonical JSON and the prev chain."""
    out, prev = [], AUDIT_ZERO
    for n, data in enumerate(files, start=1):
        plain = read_file(data, ak, sign_pub, vault_id, device_id, n, KIND_AUDIT)
        doc = json.loads(plain)
        if jcs(doc) != plain or set(doc) != {"v", "prev", "t", "ev", "d"} or doc["v"] != 1:
            raise Reject()
        if doc["prev"] != prev.hex():
            raise Reject()
        out.append(doc)
        prev = sha256(data)
    return out


# ---- device records and vault.json (crypto.md 7.4) ----
def sign_json(authority_seed: bytes, context: str, obj) -> bytes:
    return SigningKey(authority_seed).sign(f"figlock/v1/{context}".encode() + b"\x00" + jcs(obj)).signature


def verify_json(authority_pub: bytes, context: str, obj, sig: bytes) -> None:
    try:
        VerifyKey(authority_pub).verify(f"figlock/v1/{context}".encode() + b"\x00" + jcs(obj), sig)
    except BadSignatureError as e:
        raise Reject() from e


# ---- quick unlock (crypto.md 8, software variant) ----
def quick_unlock_blob(qk: bytes, auk_: bytes, vault_id: bytes, device_id: bytes, nonce: bytes) -> bytes:
    return seal(qk, auk_, aad("quick-unlock", vault_id, device_id), nonce)


# ---- quick unlock, Secure Enclave variant (ADR-0025) ----
# QK is a P-256 key agreement key. In the product its secret half never leaves the Secure Enclave; the vectors
# use a software key so the wrap around it can be checked byte for byte.
SE_VERSION = b"\x01"
SE_INFO = b"figlock quick-unlock se v1"
P256_POINT = 65


def p256_pub(secret: bytes) -> bytes:
    key = ec.derive_private_key(int.from_bytes(secret, "big"), ec.SECP256R1())
    return key.public_key().public_bytes(serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)


def p256_agree(secret: bytes, peer_pub: bytes) -> bytes:
    key = ec.derive_private_key(int.from_bytes(secret, "big"), ec.SECP256R1())
    peer = ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256R1(), peer_pub)
    return key.exchange(ec.ECDH(), peer)


def se_wrap_key(shared: bytes, eph_pub: bytes, qk_pub: bytes, vault_id: bytes, device_id: bytes) -> bytes:
    return HKDF(algorithm=hashes.SHA256(), length=32, salt=eph_pub + qk_pub,
                info=SE_INFO + vault_id + device_id).derive(shared)


def quick_unlock_se_blob(qk_pub: bytes, eph_secret: bytes, auk_: bytes, vault_id: bytes, device_id: bytes,
                         nonce: bytes) -> bytes:
    eph_pub = p256_pub(eph_secret)
    k = se_wrap_key(p256_agree(eph_secret, qk_pub), eph_pub, qk_pub, vault_id, device_id)
    return SE_VERSION + eph_pub + seal(k, auk_, aad("quick-unlock", vault_id, device_id), nonce)


def quick_unlock_se_open(qk_secret: bytes, blob: bytes, vault_id: bytes, device_id: bytes) -> bytes:
    if len(blob) < 1 + P256_POINT + 1 + 24 + 16 or blob[:1] != SE_VERSION:
        raise Reject()
    eph_pub = blob[1:1 + P256_POINT]
    try:
        shared = p256_agree(qk_secret, eph_pub)
    except ValueError as e:
        raise Reject() from e
    k = se_wrap_key(shared, eph_pub, p256_pub(qk_secret), vault_id, device_id)
    return open_(k, blob[1 + P256_POINT:], aad("quick-unlock", vault_id, device_id))


# ---- rotation (crypto.md 9.1) ----
def epoch_key(new_vk: bytes, old_vk: bytes, vault_id: bytes, old_epoch: int, new_epoch: int, nonce: bytes) -> bytes:
    return seal(new_vk, old_vk, aad("epoch-key", vault_id, u32(old_epoch), u32(new_epoch)), nonce)


# ---- public-key sealing and invites (pairing-recovery.md 3.3, 7.1) ----
def x25519_pub(priv: bytes) -> bytes:
    return X25519PrivateKey.from_private_bytes(priv).public_key().public_bytes_raw()


def x25519(priv: bytes, pub: bytes) -> bytes:
    return X25519PrivateKey.from_private_bytes(priv).exchange(X25519PublicKey.from_public_bytes(pub))


def seal_to(pub: bytes, plaintext: bytes, associated: bytes, eph_priv: bytes, nonce: bytes) -> bytes:
    eph_pub = x25519_pub(eph_priv)
    key = hkdf(x25519(eph_priv, pub), "figlock/v1/seal-to", salt=eph_pub + pub)
    return eph_pub + seal(key, plaintext, associated, nonce)


def open_seal_to(priv: bytes, blob: bytes, associated: bytes) -> bytes:
    if len(blob) < 32:
        raise Reject()
    eph_pub, envelope = blob[:32], blob[32:]
    pub = x25519_pub(priv)
    key = hkdf(x25519(priv, eph_pub), "figlock/v1/seal-to", salt=eph_pub + pub)
    return open_(key, envelope, associated)


def invite_secret_bytes(code: str) -> bytes:
    """s is a 100-bit big-endian integer in 13 bytes (top 4 bits zero)."""
    t = code.upper().replace("-", "").replace(" ", "")
    if len(t) != 20 or any(c not in CROCKFORD for c in t):
        raise Reject()
    return int("".join(f"{CROCKFORD.index(c):05b}" for c in t), 2).to_bytes(13, "big")


def invite_code_format(s: bytes) -> str:
    n = int.from_bytes(s, "big")
    assert n < 1 << 100
    t = "".join(CROCKFORD[(n >> (5 * (19 - i))) & 31] for i in range(20))
    return "-".join(t[i:i + 4] for i in range(0, 20, 4))


def invite_keys(s: bytes, vault_id: bytes, en_priv: bytes, et_pub: bytes) -> dict:
    invite_id = hkdf(s, "figlock/v1/invite-id", 16, salt=vault_id)
    shared = x25519(en_priv, et_pub)
    return {
        "invite_id": invite_id,
        "k_req": hkdf(shared + s, "figlock/v1/invite-req", salt=invite_id),
        "k_resp": hkdf(shared + s, "figlock/v1/invite-resp", salt=invite_id),
    }


# ---- sync: HLC and merge (sync.md 5, 6) ----
MAX_DRIFT_MS = 24 * 3600 * 1000


class Hlc:
    def __init__(self, l: int = 0, c: int = 0):
        self.l, self.c = l, c

    def local(self, now: int) -> tuple[int, int]:
        if now > self.l:
            self.l, self.c = now, 0
        else:
            self.c += 1
        if self.c > 0xFFFF:
            raise Reject()  # the caller waits one millisecond and retries
        return self.l, self.c

    def receive(self, lm: int, cm: int, now: int) -> tuple[int, int]:
        l2 = max(self.l, lm, now)
        if l2 == self.l == lm:
            c = max(self.c, cm) + 1
        elif l2 == self.l:
            c = self.c + 1
        elif l2 == lm:
            c = cm + 1
        else:
            c = 0
        self.l, self.c = l2, c
        return l2, c


def is_held(op_wall_ms: int, now: int) -> bool:
    return op_wall_ms > now + MAX_DRIFT_MS


def order_key(wall: int, counter: int, device_hex: str) -> tuple:
    return (wall, counter, device_hex)


def merge(ops: list[dict], purge_horizon: tuple | None = None, known_items: set | None = None) -> dict:
    """ops: field writes {item, field, wall, counter, device, value}. Returns {item: {field: [wall, counter,
    device, value]}}. Ops for unknown items older than purge_horizon (wall, counter) are dropped."""
    state: dict = {}
    for op in ops:
        if (purge_horizon is not None and known_items is not None and op["item"] not in known_items
                and (op["wall"], op["counter"]) < purge_horizon):
            continue
        cur = state.setdefault(op["item"], {}).get(op["field"])
        key = order_key(op["wall"], op["counter"], op["device"])
        if cur is None or key > order_key(cur[0], cur[1], cur[2]):
            state[op["item"]][op["field"]] = [op["wall"], op["counter"], op["device"], op["value"]]
    return state
