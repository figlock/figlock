//! Secure Enclave variant of the quick-unlock wrap (`docs/adr/0025-secure-enclave-quick-unlock.md`).
//!
//! `QK` is a P-256 key agreement key that lives in the Secure Enclave. This module is the software half: it
//! wraps the Account Unlock Key to `QK`'s public key (an ephemeral key, ECDH, HKDF, then the usual seal) and
//! unwraps it from the shared secret the platform computes with `QK`'s private half, which it only does after
//! the user is present. Nothing here touches the platform, so it is tested by the vectors in
//! `docs/spec/vectors/quick-unlock.json`.

use hkdf::Hkdf;
use p256::{PublicKey, SecretKey, ecdh::diffie_hellman, elliptic_curve::sec1::ToSec1Point};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::aead::{TAG_LEN, open, seal};
use crate::keys::qu_aad;
use crate::{Error, Key, Result};

/// Length of an uncompressed SEC1 P-256 point: `0x04 || x || y`.
pub const QUICK_SE_POINT_LEN: usize = 65;
const VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
/// Version byte, ephemeral public key, then a sealed envelope (version, nonce, ciphertext, tag) of a 32 byte key.
const MIN_BLOB: usize = 1 + QUICK_SE_POINT_LEN + 1 + NONCE_LEN + TAG_LEN;
const INFO: &[u8] = b"figlock quick-unlock se v1";

fn public_key(bytes: &[u8]) -> Result<PublicKey> {
    if bytes.len() != QUICK_SE_POINT_LEN || bytes[0] != 0x04 {
        return Err(Error);
    }
    PublicKey::from_sec1_bytes(bytes).map_err(|_| Error)
}

fn point_bytes(key: &PublicKey) -> Result<[u8; QUICK_SE_POINT_LEN]> {
    key.to_sec1_point(false)
        .as_bytes()
        .try_into()
        .map_err(|_| Error)
}

fn wrap_key(
    shared: &[u8],
    eph_public: &[u8],
    qk_public: &[u8],
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
) -> Result<Key> {
    let mut salt = Vec::with_capacity(2 * QUICK_SE_POINT_LEN);
    salt.extend_from_slice(eph_public);
    salt.extend_from_slice(qk_public);
    let mut info = Vec::with_capacity(INFO.len() + 32);
    info.extend_from_slice(INFO);
    info.extend_from_slice(vault_id);
    info.extend_from_slice(device_id);
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(&salt), shared)
        .expand(&info, out.as_mut())
        .map_err(|_| Error)?;
    Ok(Key::from_bytes(*out))
}

/// A fresh ephemeral key. A random 32 byte string is a valid P-256 scalar except with negligible probability,
/// and then we draw again.
fn random_secret() -> Result<SecretKey> {
    for _ in 0..8 {
        let mut b = Zeroizing::new([0u8; 32]);
        getrandom::fill(b.as_mut()).map_err(|_| Error)?;
        if let Ok(k) = SecretKey::from_slice(b.as_ref()) {
            return Ok(k);
        }
    }
    Err(Error)
}

/// Everything the wrap needs from the ephemeral key: its public point and the key derived with `QK`.
fn derive(
    eph: &SecretKey,
    qk_public: &[u8],
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
) -> Result<([u8; QUICK_SE_POINT_LEN], Key)> {
    let qk = public_key(qk_public)?;
    let eph_public = point_bytes(&eph.public_key())?;
    let shared = diffie_hellman(eph.to_nonzero_scalar(), qk.as_affine());
    let k = wrap_key(
        shared.raw_secret_bytes().as_slice(),
        &eph_public,
        qk_public,
        vault_id,
        device_id,
    )?;
    Ok((eph_public, k))
}

fn blob(eph_public: &[u8; QUICK_SE_POINT_LEN], envelope: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + QUICK_SE_POINT_LEN + envelope.len());
    out.push(VERSION);
    out.extend_from_slice(eph_public);
    out.extend_from_slice(envelope);
    out
}

/// Wrap `auk` to the Secure Enclave key whose public point is `qk_public`. Needs no prompt: only unwrapping
/// uses the private half.
pub fn seal_quick_unlock_se(
    qk_public: &[u8],
    auk: &Key,
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
) -> Result<Vec<u8>> {
    let eph = random_secret()?;
    let (eph_public, k) = derive(&eph, qk_public, vault_id, device_id)?;
    let envelope = seal(&k, auk.expose(), &qu_aad(vault_id, device_id))?;
    Ok(blob(&eph_public, &envelope))
}

/// Test-vector entry point; see [`crate::vector_hooks`].
#[cfg(feature = "vector-hooks")]
pub fn seal_quick_unlock_se_with(
    qk_public: &[u8],
    eph_secret: &[u8; 32],
    auk: &Key,
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
    nonce: &[u8; NONCE_LEN],
) -> Result<Vec<u8>> {
    let eph = SecretKey::from_slice(eph_secret).map_err(|_| Error)?;
    let (eph_public, k) = derive(&eph, qk_public, vault_id, device_id)?;
    let envelope =
        crate::aead::seal_with_nonce(&k, auk.expose(), &qu_aad(vault_id, device_id), nonce)?;
    Ok(blob(&eph_public, &envelope))
}

/// The ephemeral public key inside `blob`, which the platform needs to compute the shared secret with `QK`.
/// Checks the version, the length and that the point is on the curve, so a damaged blob never reaches the
/// Secure Enclave.
pub fn quick_unlock_se_ephemeral_public(blob: &[u8]) -> Result<[u8; QUICK_SE_POINT_LEN]> {
    if blob.len() < MIN_BLOB || blob[0] != VERSION {
        return Err(Error);
    }
    let point = &blob[1..1 + QUICK_SE_POINT_LEN];
    public_key(point)?;
    point.try_into().map_err(|_| Error)
}

/// Unwrap the Account Unlock Key. `shared` is the 32 byte ECDH result the platform computed with `QK` and the
/// blob's ephemeral public key; `qk_public` is `QK`'s own public point.
pub fn open_quick_unlock_se(
    blob: &[u8],
    qk_public: &[u8],
    shared: &[u8],
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
) -> Result<Key> {
    let eph_public = quick_unlock_se_ephemeral_public(blob)?;
    public_key(qk_public)?;
    let k = wrap_key(shared, &eph_public, qk_public, vault_id, device_id)?;
    let opened = open(
        &k,
        &blob[1 + QUICK_SE_POINT_LEN..],
        &qu_aad(vault_id, device_id),
    )?;
    Key::from_slice(&opened)
}
