//! The optional Recovery Key (`pairing-recovery.md` 7, `crypto.md` 4.2 and 9.3, ADR-0029): an X25519 key whose
//! public half wraps the Vault Key in `vault.json`. Its private half is written down once, as a single
//! `FLR1-` code or as SLIP-0039 shares, and never stored by FigLock.

use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::pairing::x25519_public;
use crate::secret_key::{b32_encode, b32_value, confusables, squash};
use crate::{Error, Key, Result, slip39};

const CODE_PREFIX: &str = "FLR1";
const CODE_CHARS: usize = 56;

/// The 32 byte private key of the Recovery Key. Zeroizes on drop; not `Clone`, `Debug` or `Display`.
pub struct RecoveryKey(Key);

fn check(rk: &[u8; 32]) -> [u8; 3] {
    let mut h = Sha256::new();
    h.update(b"figlock/v1/rk-check");
    h.update(rk);
    let d = h.finalize();
    [d[0], d[1], d[2]]
}

impl RecoveryKey {
    /// A fresh key from the OS CSPRNG.
    pub fn random() -> Result<Self> {
        Ok(RecoveryKey(Key::random()?))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        RecoveryKey(Key::from_bytes(bytes))
    }

    pub(crate) fn private(&self) -> &Key {
        &self.0
    }

    /// The public half, stored in `vault.json` as `rk_pub`.
    pub fn public(&self) -> [u8; 32] {
        x25519_public(&self.0)
    }

    /// `FLR1-XXXX-...`: 56 base32 characters in 14 groups, the 32 key bytes plus a 3-byte check.
    pub fn format(&self) -> Zeroizing<String> {
        let mut raw = Zeroizing::new([0u8; 35]);
        raw[..32].copy_from_slice(self.0.expose());
        raw[32..].copy_from_slice(&check(self.0.expose()));
        let s = Zeroizing::new(b32_encode(raw.as_ref()));
        let groups: Vec<&str> = (0..CODE_CHARS / 4).map(|i| &s[i * 4..i * 4 + 4]).collect();
        Zeroizing::new(format!("{CODE_PREFIX}-{}", groups.join("-")))
    }

    /// Parse the single code. A wrong check, a wrong length or a Secret Key (`FL1-`) is an error.
    pub fn parse(text: &str) -> Result<Self> {
        let squashed = Zeroizing::new(squash(text));
        // The prefix has an `L` and a `1`, so it is matched before the confusables are mapped.
        let body = Zeroizing::new(confusables(
            squashed.strip_prefix(CODE_PREFIX).ok_or(Error)?,
        ));
        if body.chars().count() != CODE_CHARS {
            return Err(Error);
        }
        let mut raw = Zeroizing::new([0u8; 35]);
        let (mut acc, mut bits, mut n) = (0u32, 0u32, 0usize);
        for c in body.chars() {
            acc = (acc << 5) | b32_value(c).ok_or(Error)?;
            bits += 5;
            while bits >= 8 {
                bits -= 8;
                raw[n] = ((acc >> bits) & 0xff) as u8;
                n += 1;
            }
            acc &= (1 << bits) - 1;
        }
        let mut rk = [0u8; 32];
        rk.copy_from_slice(&raw[..32]);
        if check(&rk) != raw[32..] {
            rk.zeroize();
            return Err(Error);
        }
        Ok(RecoveryKey::from_bytes(rk))
    }

    /// Split into `count` SLIP-0039 shares of which `threshold` recover it (`2 <= threshold <= count <= 16`).
    pub fn split(&self, threshold: u8, count: u8) -> Result<Vec<Zeroizing<String>>> {
        slip39::split(self.0.expose(), threshold, count)
    }

    /// Recover from exactly the threshold number of shares.
    pub fn from_shares(shares: &[&str]) -> Result<Self> {
        let secret = slip39::combine(shares, b"")?;
        Ok(RecoveryKey(Key::from_slice(&secret)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> RecoveryKey {
        RecoveryKey::random().unwrap()
    }

    #[test]
    fn a_code_round_trips_in_any_dress() {
        let k = key();
        let text = k.format();
        assert!(text.starts_with("FLR1-"));
        assert_eq!(text.split('-').count(), 15);
        for dressed in [
            text.to_string(),
            text.to_lowercase(),
            text.replace('-', " "),
            format!("  {}\n", text.as_str()),
        ] {
            assert_eq!(
                RecoveryKey::parse(&dressed).unwrap().0.expose(),
                k.0.expose()
            );
        }
    }

    #[test]
    fn a_typo_a_short_code_and_a_secret_key_are_refused() {
        let k = key();
        let text = k.format();
        let mut chars: Vec<char> = text.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == '0' { '1' } else { '0' };
        assert!(RecoveryKey::parse(&chars.iter().collect::<String>()).is_err());
        assert!(RecoveryKey::parse(&text[..text.len() - 5]).is_err());
        let sk = crate::SecretKey::random().unwrap();
        assert!(RecoveryKey::parse(&sk.format()).is_err());
        assert!(RecoveryKey::parse("").is_err());
    }

    #[test]
    fn shares_recover_the_same_key() {
        let k = key();
        let shares = k.split(2, 3).unwrap();
        let back = RecoveryKey::from_shares(&[&shares[2], &shares[0]]).unwrap();
        assert_eq!(back.public(), k.public());
        assert!(RecoveryKey::from_shares(&[&shares[1]]).is_err());
    }
}
