//! The generator as the window sees it. It needs no vault, so it works while locked too and holds no state;
//! the draw and the strength estimate are `figlock_core::generate`, never reimplemented in the window.

use figlock_core::generate::{
    self, Level, PassphraseOptions, PasswordOptions, Separator, Strength,
};
use serde::{Deserialize, Serialize};

use super::{CmdError, CmdRes};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PasswordSpec {
    pub length: usize,
    pub lower: bool,
    pub upper: bool,
    pub digits: bool,
    pub symbols: bool,
    pub avoid_lookalikes: bool,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum SeparatorSpec {
    Hyphen,
    Dot,
    Space,
    None,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PassphraseSpec {
    pub words: usize,
    pub separator: SeparatorSpec,
    pub capitalize: bool,
    pub add_number: bool,
}

/// A generated secret with its strength, so one call fills the whole panel.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Generated {
    pub value: String,
    pub strength: StrengthView,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct StrengthView {
    pub level: &'static str,
    pub bits: u32,
}

impl From<Strength> for StrengthView {
    fn from(s: Strength) -> Self {
        StrengthView {
            level: match s.level {
                Level::Weak => "weak",
                Level::Fair => "fair",
                Level::Strong => "strong",
            },
            bits: s.bits,
        }
    }
}

fn failed() -> CmdError {
    CmdError {
        code: "invalid",
        message: "Those options cannot make a password. Turn on at least one character type."
            .into(),
    }
}

pub fn password(spec: &PasswordSpec) -> CmdRes<Generated> {
    let value = generate::password(&PasswordOptions {
        length: spec.length,
        lower: spec.lower,
        upper: spec.upper,
        digits: spec.digits,
        symbols: spec.symbols,
        avoid_lookalikes: spec.avoid_lookalikes,
    })
    .map_err(|_| failed())?;
    Ok(Generated {
        strength: generate::password_strength(&value).into(),
        value: value.to_string(),
    })
}

pub fn passphrase(spec: &PassphraseSpec) -> CmdRes<Generated> {
    let o = PassphraseOptions {
        words: spec.words,
        separator: match spec.separator {
            SeparatorSpec::Hyphen => Separator::Hyphen,
            SeparatorSpec::Dot => Separator::Dot,
            SeparatorSpec::Space => Separator::Space,
            SeparatorSpec::None => Separator::None,
        },
        capitalize: spec.capitalize,
        add_number: spec.add_number,
    };
    let value = generate::passphrase(&o).map_err(|_| failed())?;
    Ok(Generated {
        strength: generate::passphrase_strength(&o).into(),
        value: value.to_string(),
    })
}

/// The strength of whatever is typed in a password field.
pub fn strength(value: &str) -> StrengthView {
    generate::password_strength(value).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> PasswordSpec {
        PasswordSpec {
            length: 20,
            lower: true,
            upper: true,
            digits: true,
            symbols: true,
            avoid_lookalikes: false,
        }
    }

    #[test]
    fn password_carries_its_strength() {
        let g = password(&spec()).unwrap();
        assert_eq!(g.value.len(), 20);
        assert_eq!(g.strength.level, "strong");
    }

    #[test]
    fn no_character_type_is_a_clear_error() {
        let s = PasswordSpec {
            lower: false,
            upper: false,
            digits: false,
            symbols: false,
            ..spec()
        };
        assert_eq!(password(&s).unwrap_err().code, "invalid");
    }

    #[test]
    fn passphrase_strength_follows_the_word_count() {
        let make = |words| {
            passphrase(&PassphraseSpec {
                words,
                separator: SeparatorSpec::Space,
                capitalize: false,
                add_number: false,
            })
            .unwrap()
        };
        assert_eq!(make(3).strength.level, "weak");
        assert_eq!(make(6).strength.level, "strong");
        assert_eq!(make(6).value.split(' ').count(), 6);
    }

    #[test]
    fn typed_values_are_rated() {
        assert_eq!(strength("").level, "weak");
        assert_eq!(strength("correct-Horse-battery-9-staple!").level, "strong");
    }

    #[test]
    fn the_wire_format_is_camel_case() {
        let s: PassphraseSpec = serde_json::from_str(
            r#"{"words":5,"separator":"hyphen","capitalize":true,"addNumber":false}"#,
        )
        .unwrap();
        assert!(s.capitalize && !s.add_number);
    }
}
