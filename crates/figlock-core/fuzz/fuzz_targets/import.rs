//! Fuzz the import parsers. The first byte picks the source, the rest is the file.
//!
//! Invariants: never panics; every draft could be added to a vault (valid field names for its type, values
//! within the limit); an error never quotes the input; and a notes import leaves nothing that still looks
//! like an API token outside the `token` field of a token item.
//!
//! Run: `cargo +nightly fuzz run import` from `crates/figlock-core`.

#![no_main]

use figlock_core::{
    ItemType, MAX_VALUE_BYTES, Source, detect_tokens, parse_import, validate_field,
};
use libfuzzer_sys::fuzz_target;

const SOURCES: [Source; 5] = [
    Source::Chrome,
    Source::Apple,
    Source::Bitwarden,
    Source::OnePassword,
    Source::Notes,
];

fuzz_target!(|data: &[u8]| {
    let Some((first, file)) = data.split_first() else {
        return;
    };
    let source = SOURCES[usize::from(*first) % SOURCES.len()];
    match parse_import(source, file) {
        Ok(report) => {
            for d in &report.items {
                for (name, value) in &d.fields {
                    validate_field(d.item_type, name).expect("valid field name");
                    assert!(value.len() <= MAX_VALUE_BYTES);
                    let is_token_value = d.item_type == ItemType::Token && name == "token";
                    if source == Source::Notes && !is_token_value {
                        assert!(detect_tokens(value).is_empty(), "token left in {name}");
                    }
                }
            }
        }
        Err(e) => {
            // Fixed messages only: a longer input must not reappear in its own error.
            let shown = e.to_string();
            if let Ok(text) = std::str::from_utf8(file) {
                if text.len() >= 8 {
                    assert!(!shown.contains(text));
                }
            }
        }
    }
});
