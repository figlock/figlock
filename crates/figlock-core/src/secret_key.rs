//! The 128-bit Secret Key and its display format (`crypto.md` 4.1).

use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{Error, Result};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 16 random bytes. Zeroizes on drop; not `Clone`, `Debug` or `Display`.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretKey([u8; 16]);

fn check(sk: &[u8; 16]) -> [u8; 4] {
    let mut h = Sha256::new();
    h.update(b"figlock/v1/sk-check");
    h.update(sk);
    let d = h.finalize();
    [d[0], d[1], d[2], d[3]]
}

pub(crate) fn b32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in data {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD[((acc >> bits) & 31) as usize] as char);
        }
    }
    debug_assert_eq!(bits, 0, "callers encode whole 5-bit groups");
    out
}

pub(crate) fn b32_char(v: u8) -> char {
    CROCKFORD[usize::from(v & 31)] as char
}

pub(crate) fn b32_value(c: char) -> Option<u32> {
    CROCKFORD
        .iter()
        .position(|&x| x as char == c)
        .map(|p| p as u32)
}

/// Upper case and drop dashes and spaces from a user-typed code.
pub(crate) fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Map the Crockford confusables (`I` and `L` read as `1`, `O` as `0`).
pub(crate) fn confusables(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'I' | 'L' => '1',
            'O' => '0',
            u => u,
        })
        .collect()
}

/// Normalise a user-typed code with no literal prefix (invite codes).
pub(crate) fn normalise(text: &str) -> String {
    confusables(&squash(text))
}

impl SecretKey {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        SecretKey(bytes)
    }

    /// Fresh random Secret Key from the OS CSPRNG.
    pub fn random() -> Result<Self> {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).map_err(|_| Error)?;
        Ok(SecretKey(b))
    }

    pub fn expose(&self) -> &[u8; 16] {
        &self.0
    }

    /// `FL1-XXXX-...` (32 base32 characters in 8 groups: 16 key bytes plus a 4-byte check).
    pub fn format(&self) -> Zeroizing<String> {
        let mut raw = Zeroizing::new([0u8; 20]);
        raw[..16].copy_from_slice(&self.0);
        raw[16..].copy_from_slice(&check(&self.0));
        let s = Zeroizing::new(b32_encode(raw.as_ref()));
        let groups: Vec<&str> = (0..8).map(|i| &s[i * 4..i * 4 + 4]).collect();
        Zeroizing::new(format!("FL1-{}", groups.join("-")))
    }

    /// Parse the display format. A wrong check is an error the UI reports as a typo in the Secret Key.
    pub fn parse(text: &str) -> Result<Self> {
        // The literal prefix is checked before mapping confusables, because it contains an `L` and a `1`.
        let squashed = Zeroizing::new(squash(text));
        let body = Zeroizing::new(confusables(squashed.strip_prefix("FL1").ok_or(Error)?));
        if body.chars().count() != 32 {
            return Err(Error);
        }
        let mut raw = Zeroizing::new([0u8; 20]);
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
        let mut sk = [0u8; 16];
        sk.copy_from_slice(&raw[..16]);
        if check(&sk) != raw[16..] {
            sk.zeroize();
            return Err(Error);
        }
        Ok(SecretKey(sk))
    }
}
