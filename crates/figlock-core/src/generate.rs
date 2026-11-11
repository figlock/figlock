//! Random password and passphrase generator, and the strength estimate shown next to it.
//!
//! Every draw is uniform (rejection sampling, no modulo bias) from the OS random source; a failure to get
//! randomness is an error, never a fallback. A password always contains every selected character class.
//! Passphrase words come from the EFF large wordlist (7776 words, CC BY 3.0, see `words.txt`).

use zeroize::Zeroizing;

use crate::error::{Error, Result};

pub const MIN_LEN: usize = 8;
pub const MAX_LEN: usize = 128;
pub const MIN_WORDS: usize = 3;
pub const MAX_WORDS: usize = 16;

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const SYMBOLS: &[u8] = b"!#$%&()*+,-./:;<=>?@[]^_{|}~";
/// Characters that are easy to mistake for one another when read aloud or off a screen.
const LOOKALIKES: &[u8] = b"Il1O0o";

const WORDLIST: &str = include_str!("words.txt");

/// Which characters a generated password may use. At least one class must be on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordOptions {
    pub length: usize,
    pub lower: bool,
    pub upper: bool,
    pub digits: bool,
    pub symbols: bool,
    pub avoid_lookalikes: bool,
}

impl Default for PasswordOptions {
    fn default() -> Self {
        Self {
            length: 20,
            lower: true,
            upper: true,
            digits: true,
            symbols: true,
            avoid_lookalikes: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    Hyphen,
    Dot,
    Space,
    None,
}

impl Separator {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hyphen => "-",
            Self::Dot => ".",
            Self::Space => " ",
            Self::None => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassphraseOptions {
    pub words: usize,
    pub separator: Separator,
    pub capitalize: bool,
    /// Append a two-digit number to one random word.
    pub add_number: bool,
}

impl Default for PassphraseOptions {
    fn default() -> Self {
        Self {
            words: 6,
            separator: Separator::Hyphen,
            capitalize: false,
            add_number: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Weak,
    Fair,
    Strong,
}

/// A rough guess at how hard a secret is to guess, in bits, with the label the meter shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strength {
    pub bits: u32,
    pub level: Level,
}

impl Strength {
    fn from_bits(bits: u32) -> Self {
        let level = match bits {
            0..45 => Level::Weak,
            45..72 => Level::Fair,
            _ => Level::Strong,
        };
        Self { bits, level }
    }
}

fn below(n: usize) -> Result<usize> {
    // Largest multiple of n that fits in u32; anything at or above it is redrawn.
    let n32 = u32::try_from(n).map_err(|_| Error)?;
    if n32 == 0 {
        return Err(Error);
    }
    let limit = u32::MAX - (u32::MAX % n32);
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).map_err(|_| Error)?;
        let v = u32::from_le_bytes(b);
        if v < limit {
            return Ok((v % n32) as usize);
        }
    }
}

fn class(set: &[u8], avoid_lookalikes: bool) -> Vec<u8> {
    set.iter()
        .copied()
        .filter(|b| !(avoid_lookalikes && LOOKALIKES.contains(b)))
        .collect()
}

/// A password of the requested length containing at least one character of every selected class.
pub fn password(o: &PasswordOptions) -> Result<Zeroizing<String>> {
    if !(MIN_LEN..=MAX_LEN).contains(&o.length) {
        return Err(Error);
    }
    let classes: Vec<Vec<u8>> = [
        (o.lower, LOWER),
        (o.upper, UPPER),
        (o.digits, DIGITS),
        (o.symbols, SYMBOLS),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, set)| class(set, o.avoid_lookalikes))
    .collect();
    if classes.is_empty() {
        return Err(Error);
    }
    let alphabet: Vec<u8> = classes.iter().flatten().copied().collect();
    for _ in 0..1000 {
        let mut out = Zeroizing::new(Vec::with_capacity(o.length));
        for _ in 0..o.length {
            out.push(alphabet[below(alphabet.len())?]);
        }
        if classes.iter().all(|c| out.iter().any(|b| c.contains(b))) {
            return Ok(Zeroizing::new(String::from_utf8_lossy(&out).into_owned()));
        }
    }
    Err(Error)
}

fn words() -> Vec<&'static str> {
    WORDLIST.lines().filter(|l| !l.is_empty()).collect()
}

/// Words drawn uniformly from the wordlist, joined by the separator.
pub fn passphrase(o: &PassphraseOptions) -> Result<Zeroizing<String>> {
    if !(MIN_WORDS..=MAX_WORDS).contains(&o.words) {
        return Err(Error);
    }
    let list = words();
    let mut picked: Vec<String> = Vec::with_capacity(o.words);
    for _ in 0..o.words {
        let w = list[below(list.len())?];
        picked.push(if o.capitalize {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        } else {
            w.to_owned()
        });
    }
    if o.add_number {
        let at = below(picked.len())?;
        picked[at].push_str(&(10 + below(90)?).to_string());
    }
    let joined = Zeroizing::new(picked.join(o.separator.as_str()));
    for w in &mut picked {
        zeroize::Zeroize::zeroize(w);
    }
    Ok(joined)
}

/// Strength of a password someone typed or pasted: the size of the character pool it draws on, with a
/// discount for the common shape of one word plus a few digits and a symbol.
pub fn password_strength(pw: &str) -> Strength {
    if pw.is_empty() {
        return Strength::from_bits(0);
    }
    let mut pool = 0u32;
    if pw.chars().any(|c| c.is_ascii_lowercase()) {
        pool += 26;
    }
    if pw.chars().any(|c| c.is_ascii_uppercase()) {
        pool += 26;
    }
    if pw.chars().any(|c| c.is_ascii_digit()) {
        pool += 10;
    }
    if pw.chars().any(|c| !c.is_ascii_alphanumeric()) {
        pool += 33;
    }
    let len = pw.chars().count() as f64;
    let mut bits = len * f64::from(pool.max(1)).log2();
    if is_word_plus_digits(pw) {
        bits *= 0.5;
    }
    Strength::from_bits(bits.round() as u32)
}

/// Letters, then up to four digits, then at most one of `!@#$`.
fn is_word_plus_digits(pw: &str) -> bool {
    let b = pw.as_bytes();
    let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    let digits = b[letters..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    let tail = &pw[letters + digits..];
    letters > 0 && digits <= 4 && (tail.is_empty() || matches!(tail, "!" | "@" | "#" | "$"))
}

/// Strength of a passphrase the generator makes with these options. Conservative: it counts only the
/// entropy of the words actually drawn, with no bonus for length or separators.
pub fn passphrase_strength(o: &PassphraseOptions) -> Strength {
    let per_word = (words().len() as f64).log2();
    let number = if o.add_number { 90f64.log2() } else { 0.0 };
    Strength::from_bits((o.words as f64 * per_word + number).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(length: usize) -> PasswordOptions {
        PasswordOptions {
            length,
            ..PasswordOptions::default()
        }
    }

    #[test]
    fn has_every_class_and_requested_length() {
        for _ in 0..200 {
            let p = password(&opts(MIN_LEN)).unwrap();
            assert_eq!(p.len(), MIN_LEN);
            assert!(p.bytes().any(|b| LOWER.contains(&b)));
            assert!(p.bytes().any(|b| UPPER.contains(&b)));
            assert!(p.bytes().any(|b| DIGITS.contains(&b)));
            assert!(p.bytes().any(|b| SYMBOLS.contains(&b)));
        }
    }

    #[test]
    fn only_selected_classes_appear() {
        let o = PasswordOptions {
            symbols: false,
            upper: false,
            ..opts(24)
        };
        for _ in 0..200 {
            let p = password(&o).unwrap();
            assert!(
                p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            );
        }
    }

    #[test]
    fn lookalikes_are_left_out_on_request() {
        let o = PasswordOptions {
            avoid_lookalikes: true,
            ..opts(64)
        };
        for _ in 0..200 {
            assert!(
                !password(&o)
                    .unwrap()
                    .bytes()
                    .any(|b| LOOKALIKES.contains(&b))
            );
        }
    }

    #[test]
    fn length_bounds_are_enforced() {
        assert!(password(&opts(MIN_LEN - 1)).is_err());
        assert!(password(&opts(MAX_LEN + 1)).is_err());
        assert!(password(&opts(MAX_LEN)).is_ok());
    }

    #[test]
    fn no_class_selected_is_an_error() {
        let o = PasswordOptions {
            lower: false,
            upper: false,
            digits: false,
            symbols: false,
            ..opts(20)
        };
        assert!(password(&o).is_err());
    }

    #[test]
    fn draws_differ() {
        assert_ne!(*password(&opts(32)).unwrap(), *password(&opts(32)).unwrap());
        let o = PassphraseOptions::default();
        assert_ne!(*passphrase(&o).unwrap(), *passphrase(&o).unwrap());
    }

    #[test]
    fn wordlist_is_the_full_eff_list() {
        let list = words();
        assert_eq!(list.len(), 7776);
        assert!(
            list.iter()
                .all(|w| w.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'))
        );
    }

    #[test]
    fn passphrase_shape() {
        let o = PassphraseOptions {
            words: 6,
            separator: Separator::Dot,
            capitalize: true,
            add_number: true,
        };
        let p = passphrase(&o).unwrap();
        let parts: Vec<&str> = p.split('.').collect();
        assert_eq!(parts.len(), 6);
        assert!(
            parts
                .iter()
                .all(|w| w.chars().next().unwrap().is_uppercase())
        );
        assert_eq!(
            parts
                .iter()
                .filter(|w| w.ends_with(|c: char| c.is_ascii_digit()))
                .count(),
            1
        );
    }

    #[test]
    fn passphrase_without_separator_and_bounds() {
        let o = PassphraseOptions {
            separator: Separator::None,
            ..PassphraseOptions::default()
        };
        assert!(!passphrase(&o).unwrap().contains(['.', ' ']));
        assert!(
            passphrase(&PassphraseOptions {
                words: MIN_WORDS - 1,
                ..o
            })
            .is_err()
        );
        assert!(
            passphrase(&PassphraseOptions {
                words: MAX_WORDS + 1,
                ..o
            })
            .is_err()
        );
    }

    #[test]
    fn strength_levels() {
        assert_eq!(password_strength("").level, Level::Weak);
        assert_eq!(password_strength("password1").level, Level::Weak);
        assert_eq!(password_strength("Summer2024!").level, Level::Weak);
        assert_eq!(
            password_strength(&password(&opts(24)).unwrap()).level,
            Level::Strong
        );
        let p = |words| {
            passphrase_strength(&PassphraseOptions {
                words,
                ..Default::default()
            })
        };
        assert_eq!(p(3).level, Level::Weak);
        assert_eq!(p(4).level, Level::Fair);
        assert_eq!(p(6).level, Level::Strong);
    }
}
