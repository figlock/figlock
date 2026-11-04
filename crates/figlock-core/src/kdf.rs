//! Argon2id and HKDF-SHA-256 (`crypto.md` 3, 5).

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::{Error, Key, Result};

/// Argon2id parameters stored in `vault.json` (public).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    /// Default profile at vault creation (RFC 9106 second recommended option).
    pub const DEFAULT: KdfParams = KdfParams {
        m_kib: 65_536,
        t: 3,
        p: 4,
    };
    pub const FLOOR: KdfParams = KdfParams {
        m_kib: 32_768,
        t: 2,
        p: 1,
    };
    pub const CEILING: KdfParams = KdfParams {
        m_kib: 1_048_576,
        t: 10,
        p: 16,
    };

    /// Reject anything below the floor or above the ceiling (`crypto.md` 5.1).
    pub fn check(&self) -> Result<()> {
        let (f, c) = (Self::FLOOR, Self::CEILING);
        if self.m_kib < f.m_kib
            || self.t < f.t
            || self.p < f.p
            || self.m_kib > c.m_kib
            || self.t > c.t
            || self.p > c.p
        {
            return Err(Error);
        }
        Ok(())
    }

    /// `u32(m) || u32(t) || u32(p) || lp(salt)`, used in the `VK` wrap AAD.
    pub fn bytes_with_salt(&self, salt: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.m_kib.to_be_bytes());
        out.extend_from_slice(&self.t.to_be_bytes());
        out.extend_from_slice(&self.p.to_be_bytes());
        out.extend_from_slice(
            &u32::try_from(salt.len())
                .expect("salt fits u32")
                .to_be_bytes(),
        );
        out.extend_from_slice(salt);
        out
    }
}

/// `pw_key = Argon2id(NFKD(password), salt, m, t, p)`, 32 bytes.
pub fn derive_pw_key(password: &str, salt: &[u8], params: &KdfParams) -> Result<Key> {
    params.check()?;
    let pw: Zeroizing<Vec<u8>> = Zeroizing::new(password.nfkd().collect::<String>().into_bytes());
    let p = Params::new(params.m_kib, params.t, params.p, Some(32)).map_err(|_| Error)?;
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(&pw, salt, out.as_mut())
        .map_err(|_| Error)?;
    Ok(Key::from_bytes(*out))
}

/// HKDF-SHA-256 with an explicit `figlock/v1/<purpose>` info string. `salt` of `None` is the empty salt.
pub(crate) fn hkdf_key(ikm: &[u8], salt: Option<&[u8]>, info: &str) -> Result<Key> {
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(salt, ikm)
        .expand(info.as_bytes(), out.as_mut())
        .map_err(|_| Error)?;
    Ok(Key::from_bytes(*out))
}

/// HKDF to an arbitrary length, for the vector sanity checks.
#[cfg(feature = "vector-hooks")]
pub fn hkdf_raw(
    ikm: &[u8],
    salt: Option<&[u8]>,
    info: &[u8],
    len: usize,
) -> Result<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(vec![0u8; len]);
    Hkdf::<Sha256>::new(salt, ikm)
        .expand(info, &mut out)
        .map_err(|_| Error)?;
    Ok(out)
}
