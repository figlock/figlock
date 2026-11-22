//! The password and passphrase generator of `figlock-core`, for the item editor of the apps. Generated values
//! come from the OS random source in Rust; the strength estimate looks at a typed or generated password.

use figlock_core::generate::{self, Level, PassphraseOptions, PasswordOptions, Separator};
use zeroize::Zeroizing;

use crate::error::{FigLockError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PassphraseSeparator {
    Hyphen,
    Dot,
    Space,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StrengthLevel {
    Weak,
    Fair,
    Strong,
}

/// A rough guess at how hard a secret is to guess, with the label the strength meter shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct GeneratedSecretStrength {
    pub bits: u32,
    pub level: StrengthLevel,
}

impl From<generate::Strength> for GeneratedSecretStrength {
    fn from(s: generate::Strength) -> Self {
        GeneratedSecretStrength {
            bits: s.bits,
            level: match s.level {
                Level::Weak => StrengthLevel::Weak,
                Level::Fair => StrengthLevel::Fair,
                Level::Strong => StrengthLevel::Strong,
            },
        }
    }
}

impl From<PassphraseSeparator> for Separator {
    fn from(s: PassphraseSeparator) -> Self {
        match s {
            PassphraseSeparator::Hyphen => Separator::Hyphen,
            PassphraseSeparator::Dot => Separator::Dot,
            PassphraseSeparator::Space => Separator::Space,
            PassphraseSeparator::None => Separator::None,
        }
    }
}

fn passphrase_options(
    words: u32,
    separator: PassphraseSeparator,
    capitalize: bool,
    add_number: bool,
) -> PassphraseOptions {
    PassphraseOptions {
        words: words as usize,
        separator: separator.into(),
        capitalize,
        add_number,
    }
}

/// A random password with at least one character of every chosen class. `InvalidInput` for a length outside
/// 8 to 128 or no class chosen.
#[uniffi::export]
pub fn generate_password(
    length: u32,
    lower: bool,
    upper: bool,
    digits: bool,
    symbols: bool,
    avoid_lookalikes: bool,
) -> Result<String> {
    let options = PasswordOptions {
        length: length as usize,
        lower,
        upper,
        digits,
        symbols,
        avoid_lookalikes,
    };
    let password = generate::password(&options).map_err(|_| FigLockError::InvalidInput)?;
    Ok(password.to_string())
}

/// A random passphrase of 3 to 16 words from the EFF large wordlist.
#[uniffi::export]
pub fn generate_passphrase(
    words: u32,
    separator: PassphraseSeparator,
    capitalize: bool,
    add_number: bool,
) -> Result<String> {
    let phrase = generate::passphrase(&passphrase_options(
        words, separator, capitalize, add_number,
    ))
    .map_err(|_| FigLockError::InvalidInput)?;
    Ok(phrase.to_string())
}

/// Strength of a password someone typed or generated.
#[uniffi::export]
pub fn password_strength(password: String) -> GeneratedSecretStrength {
    let password = Zeroizing::new(password);
    generate::password_strength(&password).into()
}

/// Strength of a passphrase made with these options.
#[uniffi::export]
pub fn passphrase_strength(
    words: u32,
    separator: PassphraseSeparator,
    capitalize: bool,
    add_number: bool,
) -> GeneratedSecretStrength {
    generate::passphrase_strength(&passphrase_options(
        words, separator, capitalize, add_number,
    ))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_follows_the_options() {
        let p = generate_password(24, true, false, true, false, true).unwrap();
        assert_eq!(p.len(), 24);
        assert!(
            p.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        );
        assert!(!p.contains(['l', '1', 'o', '0']));
        assert_eq!(
            generate_password(4, true, true, true, true, false),
            Err(FigLockError::InvalidInput)
        );
        assert_eq!(
            generate_password(20, false, false, false, false, false),
            Err(FigLockError::InvalidInput)
        );
    }

    #[test]
    fn passphrase_follows_the_options() {
        let p = generate_passphrase(5, PassphraseSeparator::Dot, true, false).unwrap();
        assert_eq!(p.split('.').count(), 5);
        assert!(
            p.split('.')
                .all(|w| w.chars().next().unwrap().is_uppercase())
        );
        assert_eq!(
            generate_passphrase(1, PassphraseSeparator::Dot, false, false),
            Err(FigLockError::InvalidInput)
        );
    }

    #[test]
    fn strength_rates_common_and_generated_secrets() {
        assert_eq!(
            password_strength("password1".into()).level,
            StrengthLevel::Weak
        );
        let generated = generate_password(24, true, true, true, true, false).unwrap();
        assert_eq!(password_strength(generated).level, StrengthLevel::Strong);
        let phrase = passphrase_strength(6, PassphraseSeparator::Hyphen, false, false);
        assert_eq!(phrase.level, StrengthLevel::Strong);
    }
}
