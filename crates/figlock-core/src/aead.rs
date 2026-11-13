//! XChaCha20-Poly1305 envelope: `0x01 || nonce(24) || ciphertext || tag(16)` (`crypto.md` 3).

use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead, aead::Payload};
use zeroize::Zeroizing;

use crate::{Error, Key, Result};

/// The envelope format version byte.
pub const VERSION: u8 = 1;
pub(crate) const NONCE_LEN: usize = 24;
pub(crate) const TAG_LEN: usize = 16;

fn cipher(key: &Key) -> Result<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(key.expose()).map_err(|_| Error)
}

fn seal_inner(key: &Key, plaintext: &[u8], aad: &[u8], nonce: &[u8; NONCE_LEN]) -> Result<Vec<u8>> {
    let ct = cipher(key)?
        .encrypt(
            &XNonce::try_from(&nonce[..]).map_err(|_| Error)?,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error)?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
    out.push(VERSION);
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Encrypt with a fresh random 192-bit nonce.
pub fn seal(key: &Key, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| Error)?;
    seal_inner(key, plaintext, aad, &nonce)
}

/// Test-vector entry point; see [`crate::vector_hooks`].
#[cfg(feature = "vector-hooks")]
pub fn seal_with_nonce(
    key: &Key,
    plaintext: &[u8],
    aad: &[u8],
    nonce: &[u8; 24],
) -> Result<Vec<u8>> {
    seal_inner(key, plaintext, aad, nonce)
}

/// Verify and decrypt. Fails on any mismatch and never returns partial plaintext.
pub fn open(key: &Key, envelope: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if envelope.len() < 1 + NONCE_LEN + TAG_LEN || envelope[0] != VERSION {
        return Err(Error);
    }
    let nonce = &envelope[1..1 + NONCE_LEN];
    let pt = cipher(key)?
        .decrypt(
            &XNonce::try_from(nonce).map_err(|_| Error)?,
            Payload {
                msg: &envelope[1 + NONCE_LEN..],
                aad,
            },
        )
        .map_err(|_| Error)?;
    Ok(Zeroizing::new(pt))
}
