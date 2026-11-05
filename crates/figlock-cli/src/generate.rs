//! Random password generator. Uniform over the alphabet (rejection sampling, no modulo bias) and always
//! contains every selected character class.

use zeroize::Zeroizing;

use crate::error::{Fail, Res};

pub const MIN_LEN: usize = 8;
pub const MAX_LEN: usize = 128;

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const SYMBOLS: &[u8] = b"!#$%&()*+,-./:;<=>?@[]^_{|}~";

fn below(n: usize) -> Res<usize> {
    // Largest multiple of n that fits in u32; anything at or above it is redrawn.
    let n32 = u32::try_from(n).map_err(|_| Fail::new("alphabet too large"))?;
    let limit = u32::MAX - (u32::MAX % n32);
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).map_err(|_| Fail::new("the system random source failed"))?;
        let v = u32::from_le_bytes(b);
        if v < limit {
            return Ok((v % n32) as usize);
        }
    }
}

pub fn password(len: usize, symbols: bool) -> Res<Zeroizing<String>> {
    if !(MIN_LEN..=MAX_LEN).contains(&len) {
        return Err(Fail::new(format!(
            "length must be between {MIN_LEN} and {MAX_LEN}"
        )));
    }
    let mut classes: Vec<&[u8]> = vec![LOWER, UPPER, DIGITS];
    if symbols {
        classes.push(SYMBOLS);
    }
    let alphabet: Vec<u8> = classes.iter().flat_map(|c| c.iter().copied()).collect();
    for _ in 0..1000 {
        let mut out = Zeroizing::new(Vec::with_capacity(len));
        for _ in 0..len {
            out.push(alphabet[below(alphabet.len())?]);
        }
        if classes.iter().all(|c| out.iter().any(|b| c.contains(b))) {
            return Ok(Zeroizing::new(String::from_utf8_lossy(&out).into_owned()));
        }
    }
    Err(Fail::new("could not generate a password"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_every_class_and_requested_length() {
        for _ in 0..200 {
            let p = password(MIN_LEN, true).unwrap();
            assert_eq!(p.len(), MIN_LEN);
            assert!(p.bytes().any(|b| LOWER.contains(&b)));
            assert!(p.bytes().any(|b| UPPER.contains(&b)));
            assert!(p.bytes().any(|b| DIGITS.contains(&b)));
            assert!(p.bytes().any(|b| SYMBOLS.contains(&b)));
        }
    }

    #[test]
    fn no_symbols_means_alphanumeric_only() {
        for _ in 0..200 {
            assert!(
                password(20, false)
                    .unwrap()
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric())
            );
        }
    }

    #[test]
    fn length_bounds_are_enforced() {
        assert!(password(MIN_LEN - 1, true).is_err());
        assert!(password(MAX_LEN + 1, true).is_err());
        assert!(password(MAX_LEN, true).is_ok());
    }

    #[test]
    fn draws_differ() {
        assert_ne!(*password(32, true).unwrap(), *password(32, true).unwrap());
    }
}
