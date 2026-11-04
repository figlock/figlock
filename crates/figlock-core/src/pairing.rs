//! Invites, public-key sealing and the Recovery Key wrap (`pairing-recovery.md` 3.3 and 7.1).

use subtle::ConstantTimeEq;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::aead::{open, seal};
use crate::kdf::hkdf_key;
use crate::secret_key::{b32_char, b32_value, normalise};
use crate::{Error, Key, Result};

const INVITE_BYTES: usize = 13;

/// `X25519` public key for a private key.
pub fn x25519_public(private: &Key) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(*private.expose())).to_bytes()
}

fn dh(private: &Key, public: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>> {
    let shared = StaticSecret::from(*private.expose()).diffie_hellman(&PublicKey::from(*public));
    // An all-zero result means a low-order public key: refuse.
    if bool::from(shared.as_bytes().ct_eq(&[0u8; 32])) {
        return Err(Error);
    }
    Ok(Zeroizing::new(*shared.as_bytes()))
}

/// Keys derived for one invite. Both sides derive the same values.
pub struct InviteKeys {
    pub invite_id: [u8; 16],
    pub k_req: Key,
    pub k_resp: Key,
}

/// `invite_id = HKDF(s, salt = vault_id, "figlock/v1/invite-id", 16)`; `K_req` and `K_resp` from
/// `X25519(my_priv, their_pub) || s` with `salt = invite_id`.
pub fn invite_keys(
    s: &[u8; INVITE_BYTES],
    vault_id: &[u8; 16],
    my_priv: &Key,
    their_pub: &[u8; 32],
) -> Result<InviteKeys> {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let mut invite_id = [0u8; 16];
    Hkdf::<Sha256>::new(Some(vault_id), s)
        .expand(b"figlock/v1/invite-id", &mut invite_id)
        .map_err(|_| Error)?;
    let shared = dh(my_priv, their_pub)?;
    let mut ikm = Zeroizing::new([0u8; 32 + INVITE_BYTES]);
    ikm[..32].copy_from_slice(shared.as_ref());
    ikm[32..].copy_from_slice(s);
    Ok(InviteKeys {
        invite_id,
        k_req: hkdf_key(ikm.as_ref(), Some(&invite_id), "figlock/v1/invite-req")?,
        k_resp: hkdf_key(ikm.as_ref(), Some(&invite_id), "figlock/v1/invite-resp")?,
    })
}

/// Show `s` (100 bits in 13 bytes, top 4 bits zero) as `XXXX-XXXX-XXXX-XXXX-XXXX`.
pub fn invite_code_format(s: &[u8; INVITE_BYTES]) -> Result<String> {
    if s[0] > 0x0f {
        return Err(Error);
    }
    let mut buf = [0u8; 16];
    buf[16 - INVITE_BYTES..].copy_from_slice(s);
    let n = u128::from_be_bytes(buf);
    let t: String = (0..20)
        .map(|i| b32_char(((n >> (5 * (19 - i))) & 31) as u8))
        .collect();
    Ok((0..5)
        .map(|i| &t[i * 4..i * 4 + 4])
        .collect::<Vec<_>>()
        .join("-"))
}

pub fn invite_code_parse(code: &str) -> Result<[u8; INVITE_BYTES]> {
    let t = normalise(code);
    if t.chars().count() != 20 {
        return Err(Error);
    }
    let mut n: u128 = 0;
    for c in t.chars() {
        n = (n << 5) | u128::from(b32_value(c).ok_or(Error)?);
    }
    let bytes = n.to_be_bytes();
    let mut out = [0u8; INVITE_BYTES];
    out.copy_from_slice(&bytes[16 - INVITE_BYTES..]);
    Ok(out)
}

fn seal_to_inner(
    public: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
    eph: &Key,
    nonce: Option<&[u8; 24]>,
) -> Result<Vec<u8>> {
    let eph_pub = x25519_public(eph);
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(&eph_pub);
    salt[32..].copy_from_slice(public);
    let key = hkdf_key(dh(eph, public)?.as_ref(), Some(&salt), "figlock/v1/seal-to")?;
    let env = match nonce {
        None => seal(&key, plaintext, aad)?,
        #[cfg(feature = "vector-hooks")]
        Some(n) => crate::aead::seal_with_nonce(&key, plaintext, aad, n)?,
        #[cfg(not(feature = "vector-hooks"))]
        Some(_) => return Err(Error),
    };
    let mut out = eph_pub.to_vec();
    out.extend_from_slice(&env);
    Ok(out)
}

/// Encrypt to an X25519 public key with a fresh ephemeral key: `eph_pub || seal(...)`.
pub fn seal_to(public: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    seal_to_inner(public, plaintext, aad, &Key::random()?, None)
}

/// Test-vector entry point with an explicit ephemeral key and nonce.
#[cfg(feature = "vector-hooks")]
pub fn seal_to_with(
    public: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
    eph: &Key,
    nonce: &[u8; 24],
) -> Result<Vec<u8>> {
    seal_to_inner(public, plaintext, aad, eph, Some(nonce))
}

pub fn open_seal_to(private: &Key, blob: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if blob.len() < 32 {
        return Err(Error);
    }
    let eph_pub: [u8; 32] = blob[..32].try_into().map_err(|_| Error)?;
    let my_pub = x25519_public(private);
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(&eph_pub);
    salt[32..].copy_from_slice(&my_pub);
    let key = hkdf_key(
        dh(private, &eph_pub)?.as_ref(),
        Some(&salt),
        "figlock/v1/seal-to",
    )?;
    open(&key, &blob[32..], aad)
}
