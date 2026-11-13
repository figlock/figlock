//! SLIP-0039 Shamir secret sharing with mnemonic shares (`pairing-recovery.md` 7.2, ADR-0028).
//!
//! Follows the specification and the reference implementation (`trezor/python-shamir-mnemonic`): a master
//! secret is encrypted with a four round Feistel cipher over PBKDF2-HMAC-SHA256, split with Shamir's scheme
//! over GF(256) (optionally in groups) and written as 10 bit words from a fixed list with an RS1024 checksum.
//! Every one of the 45 official vectors is run by `tests/slip39.rs`.
//!
//! FigLock makes one group with threshold `M` of `N` and an empty passphrase ([`split`]); [`combine`] accepts
//! the whole format so shares from other tools recover too. Like the rest of the crate, every failure is the
//! one generic [`Error`]. Share words and secrets live in zeroizing buffers, and field arithmetic uses no
//! tables, so no secret byte is ever used as a memory index.

use std::sync::OnceLock;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use crate::{Error, Result};

const RADIX_BITS: usize = 10;
const ID_LENGTH_BITS: u32 = 15;
const EXTENDABLE_BITS: u32 = 1;
const ITERATION_EXP_BITS: u32 = 4;
const ID_EXP_WORDS: usize = 2;
const CHECKSUM_WORDS: usize = 3;
const METADATA_WORDS: usize = ID_EXP_WORDS + 2 + CHECKSUM_WORDS;
const MIN_STRENGTH_BITS: usize = 128;
const MIN_MNEMONIC_WORDS: usize = METADATA_WORDS + MIN_STRENGTH_BITS.div_ceil(RADIX_BITS);
const DIGEST_BYTES: usize = 4;
const BASE_ITERATIONS: u32 = 10_000;
const ROUNDS: u8 = 4;
const SECRET_INDEX: u8 = 255;
const DIGEST_INDEX: u8 = 254;
/// Most shares one group can have (the index is 4 bits).
pub const MAX_SHARES: usize = 16;
/// Words in a share of a 256 bit secret: what a Recovery Key card holds.
pub const WORDS_FOR_256_BITS: usize = METADATA_WORDS + 256_usize.div_ceil(RADIX_BITS);
/// The Feistel work factor FigLock uses: `10000 << 1` PBKDF2 iterations over the four rounds.
const ITERATION_EXPONENT: u8 = 1;

const CUSTOM_ORIGINAL: &[u8] = b"shamir";
const CUSTOM_EXTENDABLE: &[u8] = b"shamir_extendable";

fn wordlist() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| include_str!("slip39_words.txt").lines().collect())
}

fn word_index(word: &str) -> Option<u16> {
    // The list is sorted, so a lookup is a binary search.
    wordlist()
        .binary_search(&word)
        .ok()
        .and_then(|i| u16::try_from(i).ok())
}

// ---- RS1024 checksum ----

fn polymod(values: impl Iterator<Item = u32>) -> u32 {
    const GEN: [u32; 10] = [
        0x00E0_E040,
        0x01C1_C080,
        0x0383_8100,
        0x0707_0200,
        0x0E0E_0009,
        0x1C0C_2412,
        0x3808_6C24,
        0x3090_FC48,
        0x21B1_F890,
        0x03F3_F120,
    ];
    let mut chk = 1u32;
    for v in values {
        let b = chk >> 20;
        chk = ((chk & 0xF_FFFF) << 10) ^ v;
        for (i, g) in GEN.iter().enumerate() {
            if (b >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

fn customization(extendable: bool) -> &'static [u8] {
    if extendable {
        CUSTOM_EXTENDABLE
    } else {
        CUSTOM_ORIGINAL
    }
}

fn checksum(data: &[u16], extendable: bool) -> [u16; CHECKSUM_WORDS] {
    let values = customization(extendable)
        .iter()
        .map(|b| u32::from(*b))
        .chain(data.iter().map(|w| u32::from(*w)))
        .chain([0u32; CHECKSUM_WORDS]);
    let p = polymod(values) ^ 1;
    let mut out = [0u16; CHECKSUM_WORDS];
    for (i, w) in out.iter_mut().enumerate() {
        *w = ((p >> (10 * (CHECKSUM_WORDS - 1 - i))) & 0x3FF) as u16;
    }
    out
}

fn checksum_ok(data: &[u16], extendable: bool) -> bool {
    polymod(
        customization(extendable)
            .iter()
            .map(|b| u32::from(*b))
            .chain(data.iter().map(|w| u32::from(*w))),
    ) == 1
}

// ---- GF(256), polynomial x^8 + x^4 + x^3 + x + 1, without tables ----

fn gf_mul(a: u8, b: u8) -> u8 {
    let (mut a, mut b, mut p) = (a, b, 0u8);
    for _ in 0..8 {
        p ^= a & 0u8.wrapping_sub(b & 1);
        let carry = a >> 7;
        a = (a << 1) ^ (0x1B & 0u8.wrapping_sub(carry));
        b >>= 1;
    }
    p
}

/// `a^254`, the inverse of a non-zero `a`.
fn gf_inv(a: u8) -> u8 {
    let mut result = 1u8;
    let mut base = a;
    let mut e = 254u8;
    while e > 0 {
        if e & 1 == 1 {
            result = gf_mul(result, base);
        }
        base = gf_mul(base, base);
        e >>= 1;
    }
    result
}

#[derive(Clone)]
struct RawShare {
    x: u8,
    data: Zeroizing<Vec<u8>>,
}

/// The value at `x` of the polynomial through `shares` (Lagrange interpolation, byte by byte).
fn interpolate(shares: &[RawShare], x: u8) -> Result<Zeroizing<Vec<u8>>> {
    let len = shares.first().ok_or(Error)?.data.len();
    for (i, s) in shares.iter().enumerate() {
        if s.data.len() != len || shares[..i].iter().any(|o| o.x == s.x) {
            return Err(Error);
        }
    }
    if let Some(s) = shares.iter().find(|s| s.x == x) {
        return Ok(s.data.clone());
    }
    let mut out = Zeroizing::new(vec![0u8; len]);
    for s in shares {
        // The basis polynomial at `x` depends on the public indices only.
        let mut basis = 1u8;
        for o in shares.iter().filter(|o| o.x != s.x) {
            basis = gf_mul(basis, gf_mul(x ^ o.x, gf_inv(s.x ^ o.x)));
        }
        for (acc, y) in out.iter_mut().zip(s.data.iter()) {
            *acc ^= gf_mul(basis, *y);
        }
    }
    Ok(out)
}

fn digest(random_part: &[u8], secret: &[u8]) -> [u8; DIGEST_BYTES] {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(random_part).expect("any key length");
    mac.update(secret);
    let full = mac.finalize().into_bytes();
    let mut out = [0u8; DIGEST_BYTES];
    out.copy_from_slice(&full[..DIGEST_BYTES]);
    out
}

fn random_bytes(n: usize) -> Result<Zeroizing<Vec<u8>>> {
    let mut b = Zeroizing::new(vec![0u8; n]);
    getrandom::fill(&mut b).map_err(|_| Error)?;
    Ok(b)
}

fn split_secret(threshold: u8, count: u8, secret: &[u8]) -> Result<Vec<RawShare>> {
    if threshold < 1 || threshold > count || usize::from(count) > MAX_SHARES {
        return Err(Error);
    }
    // With a threshold of 1 the digest is not used and every share is the secret.
    if threshold == 1 {
        return Ok((0..count)
            .map(|x| RawShare {
                x,
                data: Zeroizing::new(secret.to_vec()),
            })
            .collect());
    }
    let random_count = threshold - 2;
    let mut shares: Vec<RawShare> = Vec::new();
    for x in 0..random_count {
        shares.push(RawShare {
            x,
            data: random_bytes(secret.len())?,
        });
    }
    let random_part = random_bytes(secret.len() - DIGEST_BYTES)?;
    let mut digest_data = Zeroizing::new(digest(&random_part, secret).to_vec());
    digest_data.extend_from_slice(&random_part);
    let mut base = shares.clone();
    base.push(RawShare {
        x: DIGEST_INDEX,
        data: digest_data,
    });
    base.push(RawShare {
        x: SECRET_INDEX,
        data: Zeroizing::new(secret.to_vec()),
    });
    for x in random_count..count {
        shares.push(RawShare {
            x,
            data: interpolate(&base, x)?,
        });
    }
    Ok(shares)
}

fn recover_secret(threshold: u8, shares: &[RawShare]) -> Result<Zeroizing<Vec<u8>>> {
    if threshold == 1 {
        return Ok(shares.first().ok_or(Error)?.data.clone());
    }
    let secret = interpolate(shares, SECRET_INDEX)?;
    let digest_share = interpolate(shares, DIGEST_INDEX)?;
    if digest_share.len() < DIGEST_BYTES {
        return Err(Error);
    }
    let (want, random_part) = digest_share.split_at(DIGEST_BYTES);
    if !bool::from(want.ct_eq(&digest(random_part, &secret))) {
        return Err(Error);
    }
    Ok(secret)
}

// ---- the Feistel cipher over the master secret ----

fn printable(passphrase: &[u8]) -> bool {
    passphrase.iter().all(|c| (32..=126).contains(c))
}

fn salt(identifier: u16, extendable: bool) -> Vec<u8> {
    if extendable {
        Vec::new()
    } else {
        let mut s = CUSTOM_ORIGINAL.to_vec();
        s.extend_from_slice(&identifier.to_be_bytes());
        s
    }
}

fn round_function(
    i: u8,
    passphrase: &[u8],
    exponent: u8,
    salt: &[u8],
    r: &[u8],
) -> Zeroizing<Vec<u8>> {
    let mut password = Zeroizing::new(vec![i]);
    password.extend_from_slice(passphrase);
    let mut full_salt = salt.to_vec();
    full_salt.extend_from_slice(r);
    let iterations = (BASE_ITERATIONS << exponent) / u32::from(ROUNDS);
    let mut out = Zeroizing::new(vec![0u8; r.len()]);
    pbkdf2::pbkdf2_hmac::<Sha256>(&password, &full_salt, iterations, &mut out);
    out
}

fn feistel(
    data: &[u8],
    passphrase: &[u8],
    exponent: u8,
    identifier: u16,
    extendable: bool,
    decrypt: bool,
) -> Result<Zeroizing<Vec<u8>>> {
    if !data.len().is_multiple_of(2) || !printable(passphrase) {
        return Err(Error);
    }
    let half = data.len() / 2;
    let mut l = Zeroizing::new(data[..half].to_vec());
    let mut r = Zeroizing::new(data[half..].to_vec());
    let salt = salt(identifier, extendable);
    let order: Vec<u8> = if decrypt {
        (0..ROUNDS).rev().collect()
    } else {
        (0..ROUNDS).collect()
    };
    for i in order {
        let f = round_function(i, passphrase, exponent, &salt, &r);
        let next: Vec<u8> = l.iter().zip(f.iter()).map(|(a, b)| a ^ b).collect();
        l = std::mem::replace(&mut r, Zeroizing::new(next));
    }
    let mut out = Zeroizing::new(r.to_vec());
    out.extend_from_slice(&l);
    Ok(out)
}

// ---- the share format ----

struct Share {
    identifier: u16,
    extendable: bool,
    iteration_exponent: u8,
    group_index: u8,
    group_threshold: u8,
    group_count: u8,
    index: u8,
    member_threshold: u8,
    value: Zeroizing<Vec<u8>>,
}

/// `bytes` as 10 bit words, big endian, with leading zero bits to fill the first word.
fn bytes_to_words(bytes: &[u8]) -> Vec<u16> {
    let count = (bytes.len() * 8).div_ceil(RADIX_BITS);
    let pad = count * RADIX_BITS - bytes.len() * 8;
    let mut bits = vec![false; pad];
    for b in bytes {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1 == 1);
        }
    }
    let words = bits
        .chunks(RADIX_BITS)
        .map(|c| c.iter().fold(0u16, |acc, bit| (acc << 1) | u16::from(*bit)))
        .collect();
    bits.zeroize();
    words
}

/// The inverse of [`bytes_to_words`]. `None` when the padding bits are not all zero.
fn words_to_bytes(words: &[u16], pad: usize) -> Option<Zeroizing<Vec<u8>>> {
    let mut bits: Vec<bool> = Vec::with_capacity(words.len() * RADIX_BITS);
    for w in words {
        for i in (0..RADIX_BITS).rev() {
            bits.push((w >> i) & 1 == 1);
        }
    }
    let ok = bits[..pad].iter().all(|b| !b);
    let bytes = bits[pad..]
        .chunks(8)
        .map(|c| c.iter().fold(0u8, |acc, bit| (acc << 1) | u8::from(*bit)))
        .collect();
    bits.zeroize();
    ok.then_some(Zeroizing::new(bytes))
}

impl Share {
    fn words(&self) -> Zeroizing<Vec<u16>> {
        let id_exp = (u32::from(self.identifier) << (EXTENDABLE_BITS + ITERATION_EXP_BITS))
            | (u32::from(self.extendable) << ITERATION_EXP_BITS)
            | u32::from(self.iteration_exponent);
        let params = (u32::from(self.group_index) << 16)
            | (u32::from(self.group_threshold - 1) << 12)
            | (u32::from(self.group_count - 1) << 8)
            | (u32::from(self.index) << 4)
            | u32::from(self.member_threshold - 1);
        let mut data = Vec::new();
        for v in [id_exp, params] {
            data.push(((v >> 10) & 0x3FF) as u16);
            data.push((v & 0x3FF) as u16);
        }
        data.extend(bytes_to_words(&self.value));
        let sum = checksum(&data, self.extendable);
        data.extend(sum);
        Zeroizing::new(data)
    }

    fn mnemonic(&self) -> Zeroizing<String> {
        let list = wordlist();
        let words = self.words();
        let mut out = Zeroizing::new(String::new());
        for (i, w) in words.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(list[usize::from(*w)]);
        }
        out
    }

    fn parse(mnemonic: &str) -> Result<Share> {
        let mut data: Zeroizing<Vec<u16>> = Zeroizing::new(Vec::new());
        for word in mnemonic.split_whitespace() {
            data.push(word_index(&word.to_ascii_lowercase()).ok_or(Error)?);
        }
        if data.len() < MIN_MNEMONIC_WORDS {
            return Err(Error);
        }
        let pad = (RADIX_BITS * (data.len() - METADATA_WORDS)) % 16;
        if pad > 8 {
            return Err(Error);
        }
        let id_exp = (u32::from(data[0]) << 10) | u32::from(data[1]);
        let identifier = (id_exp >> (EXTENDABLE_BITS + ITERATION_EXP_BITS)) as u16;
        let extendable = (id_exp >> ITERATION_EXP_BITS) & 1 == 1;
        let iteration_exponent = (id_exp & ((1 << ITERATION_EXP_BITS) - 1)) as u8;
        if !checksum_ok(&data, extendable) {
            return Err(Error);
        }
        let params = (u32::from(data[2]) << 10) | u32::from(data[3]);
        let nibble = |shift: u32| ((params >> shift) & 0xF) as u8;
        let (group_index, group_threshold, group_count, index, member_threshold) = (
            nibble(16),
            nibble(12) + 1,
            nibble(8) + 1,
            nibble(4),
            nibble(0) + 1,
        );
        if group_count < group_threshold {
            return Err(Error);
        }
        let value = words_to_bytes(&data[ID_EXP_WORDS + 2..data.len() - CHECKSUM_WORDS], pad)
            .ok_or(Error)?;
        debug_assert!(identifier < (1 << ID_LENGTH_BITS));
        Ok(Share {
            identifier,
            extendable,
            iteration_exponent,
            group_index,
            group_threshold,
            group_count,
            index,
            member_threshold,
            value,
        })
    }

    /// What every share of one backup agrees on.
    fn common(&self) -> (u16, bool, u8, u8, u8) {
        (
            self.identifier,
            self.extendable,
            self.iteration_exponent,
            self.group_threshold,
            self.group_count,
        )
    }
}

fn random_identifier() -> Result<u16> {
    let mut b = [0u8; 2];
    getrandom::fill(&mut b).map_err(|_| Error)?;
    Ok(u16::from_be_bytes(b) & ((1 << ID_LENGTH_BITS) - 1))
}

/// What one card says about its set, so a screen can tell the person how many more to enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShareInfo {
    /// Cards of this card's group needed to recover.
    pub member_threshold: u8,
    /// Groups needed. FigLock makes one group; cards from other tools may have more.
    pub group_threshold: u8,
    /// This card's number within its group, from 1.
    pub number: u8,
}

/// Read the metadata of one mnemonic. The words are checked (list, checksum, padding) but nothing is recovered.
pub fn info(mnemonic: &str) -> Result<ShareInfo> {
    let s = Share::parse(mnemonic)?;
    Ok(ShareInfo {
        member_threshold: s.member_threshold,
        group_threshold: s.group_threshold,
        number: s.index + 1,
    })
}

/// Split `master_secret` (16 to 32 bytes in steps of two) into `groups` of `(member_threshold, member_count)`,
/// of which `group_threshold` are needed. Returns the mnemonics of each group.
pub fn generate(
    group_threshold: u8,
    groups: &[(u8, u8)],
    master_secret: &[u8],
    passphrase: &[u8],
) -> Result<Vec<Vec<Zeroizing<String>>>> {
    if master_secret.len() * 8 < MIN_STRENGTH_BITS
        || usize::from(group_threshold) > groups.len()
        || groups.is_empty()
        || groups.len() > MAX_SHARES
        || groups.iter().any(|(m, n)| *m == 1 && *n > 1)
    {
        return Err(Error);
    }
    let identifier = random_identifier()?;
    let encrypted = feistel(
        master_secret,
        passphrase,
        ITERATION_EXPONENT,
        identifier,
        true,
        false,
    )?;
    let group_secrets = split_secret(group_threshold, groups.len() as u8, &encrypted)?;
    let mut out = Vec::new();
    for ((member_threshold, member_count), group) in groups.iter().zip(group_secrets) {
        let members = split_secret(*member_threshold, *member_count, &group.data)?;
        out.push(
            members
                .into_iter()
                .map(|m| {
                    Share {
                        identifier,
                        extendable: true,
                        iteration_exponent: ITERATION_EXPONENT,
                        group_index: group.x,
                        group_threshold,
                        group_count: groups.len() as u8,
                        index: m.x,
                        member_threshold: *member_threshold,
                        value: m.data,
                    }
                    .mnemonic()
                })
                .collect(),
        );
    }
    Ok(out)
}

/// The shares of one group with threshold `threshold` of `count` for a secret of 16 to 32 bytes: what FigLock
/// prints on its Recovery Key cards. A threshold of one is refused for more than one card, like the reference.
pub fn split(secret: &[u8], threshold: u8, count: u8) -> Result<Vec<Zeroizing<String>>> {
    if !(2..=MAX_SHARES as u8).contains(&count) || threshold < 2 {
        return Err(Error);
    }
    generate(1, &[(threshold, count)], secret, b"")?
        .into_iter()
        .next()
        .ok_or(Error)
}

/// Recover the master secret from `mnemonics`. The shares must be exactly those needed: `group_threshold`
/// groups with exactly `member_threshold` shares each. Identical shares count once.
pub fn combine(mnemonics: &[&str], passphrase: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if mnemonics.is_empty() {
        return Err(Error);
    }
    let mut parsed = Vec::new();
    for m in mnemonics {
        parsed.push(Share::parse(m)?);
    }
    let common = parsed[0].common();
    if parsed.iter().any(|s| s.common() != common) {
        return Err(Error);
    }
    let (identifier, extendable, iteration_exponent, group_threshold, _) = common;

    // Group by index; a repeated share is the same share, not a second one.
    let mut groups: std::collections::BTreeMap<u8, Vec<Share>> = std::collections::BTreeMap::new();
    for s in parsed {
        let group = groups.entry(s.group_index).or_default();
        if let Some(first) = group.first()
            && first.member_threshold != s.member_threshold
        {
            return Err(Error);
        }
        if let Some(same) = group.iter().find(|o| o.index == s.index) {
            if *same.value != *s.value {
                return Err(Error);
            }
            continue;
        }
        group.push(s);
    }
    if groups.len() != usize::from(group_threshold) {
        return Err(Error);
    }
    let mut group_shares = Vec::new();
    for (group_index, members) in &groups {
        let threshold = members[0].member_threshold;
        if members.len() != usize::from(threshold) {
            return Err(Error);
        }
        let raw: Vec<RawShare> = members
            .iter()
            .map(|m| RawShare {
                x: m.index,
                data: m.value.clone(),
            })
            .collect();
        group_shares.push(RawShare {
            x: *group_index,
            data: recover_secret(threshold, &raw)?,
        });
    }
    let encrypted = recover_secret(group_threshold, &group_shares)?;
    feistel(
        &encrypted,
        passphrase,
        iteration_exponent,
        identifier,
        extendable,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wordlist_has_1024_sorted_unique_words() {
        let w = wordlist();
        assert_eq!(w.len(), 1024);
        assert!(w.windows(2).all(|p| p[0] < p[1]));
        // The first four letters identify a word, so a typo can be matched on them.
        let mut four: Vec<&str> = w.iter().map(|s| &s[..4.min(s.len())]).collect();
        four.dedup();
        assert_eq!(four.len(), 1024);
    }

    #[test]
    fn field_arithmetic_is_the_aes_field() {
        // FIPS 197 worked example: {57} x {83} = {c1}.
        assert_eq!(gf_mul(0x57, 0x83), 0xC1);
        for a in 1..=255u8 {
            assert_eq!(gf_mul(a, gf_inv(a)), 1, "{a}");
        }
        assert_eq!(gf_mul(0, 7), 0);
        assert_eq!(gf_mul(1, 0xAB), 0xAB);
    }

    #[test]
    fn words_and_bytes_convert_both_ways() {
        // Secrets are an even number of bytes, which is what makes the padding fit in 8 bits.
        for len in [16usize, 18, 20, 32] {
            let bytes: Vec<u8> = (0..len as u8).map(|i| i.wrapping_mul(37)).collect();
            let words = bytes_to_words(&bytes);
            let pad = (words.len() * RADIX_BITS) % 16;
            assert_eq!(*words_to_bytes(&words, pad).unwrap(), bytes);
        }
    }

    #[test]
    fn a_256_bit_share_has_33_words() {
        let secret = [7u8; 32];
        let shares = split(&secret, 2, 3).unwrap();
        assert_eq!(shares.len(), 3);
        for s in &shares {
            assert_eq!(s.split(' ').count(), WORDS_FOR_256_BITS);
        }
        assert_eq!(WORDS_FOR_256_BITS, 33);
    }
}
