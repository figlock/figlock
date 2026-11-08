//! File contents for document items (`docs/adr/0024-document-items.md`).
//!
//! A file is stored as one text field, `file/<id>/data`, holding standard base64 with padding. Fields are
//! CBOR text everywhere else, so this adds no value kind and changes no format. The text field is capped at
//! [`MAX_VALUE_BYTES`], which leaves room for exactly [`MAX_FILE_BYTES`] of file.

use base64::{Engine, engine::general_purpose::STANDARD};
use zeroize::Zeroizing;

use crate::{Error, MAX_VALUE_BYTES, Result};

/// Largest file a document item holds: 768 KiB, which encodes to exactly [`MAX_VALUE_BYTES`] of base64.
pub const MAX_FILE_BYTES: usize = MAX_VALUE_BYTES / 4 * 3;

/// Encode file contents for a `file/<id>/data` field. Fails when the file is empty or over the cap.
pub fn encode_file(bytes: &[u8]) -> Result<Zeroizing<String>> {
    if bytes.is_empty() || bytes.len() > MAX_FILE_BYTES {
        return Err(Error);
    }
    let mut out = Zeroizing::new(String::with_capacity(bytes.len().div_ceil(3) * 4));
    STANDARD.encode_string(bytes, &mut out);
    Ok(out)
}

/// Decode a `file/<id>/data` value. Strict: canonical padding only, no whitespace, no trailing bits.
pub fn decode_file(text: &str) -> Result<Zeroizing<Vec<u8>>> {
    if text.is_empty() || text.len() > MAX_VALUE_BYTES {
        return Err(Error);
    }
    // Decode into a buffer of the final size so no reallocation leaves a stray copy behind.
    let mut out = Zeroizing::new(vec![0u8; text.len() / 4 * 3]);
    let n = STANDARD
        .decode_slice(text.as_bytes(), &mut out)
        .map_err(|_| Error)?;
    out.truncate(n);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_length_up_to_a_few_blocks() {
        for len in 1..=40usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 7 + 3) as u8).collect();
            let text = encode_file(&data).unwrap();
            assert_eq!(&**decode_file(&text).unwrap(), &data[..]);
        }
    }

    #[test]
    fn the_cap_is_exact() {
        assert_eq!(MAX_FILE_BYTES, 786_432);
        let max = vec![0xa5u8; MAX_FILE_BYTES];
        let text = encode_file(&max).unwrap();
        assert_eq!(text.len(), MAX_VALUE_BYTES);
        assert_eq!(decode_file(&text).unwrap().len(), MAX_FILE_BYTES);
        assert!(encode_file(&vec![0u8; MAX_FILE_BYTES + 1]).is_err());
        assert!(encode_file(&[]).is_err());
    }

    #[test]
    fn decode_rejects_anything_not_canonical() {
        for bad in [
            "", "AAA",      // missing padding
            "AA==\n",   // whitespace
            "AA =",     // inner space
            "AAB=",     // non-zero trailing bits
            "AA==AA==", // padding in the middle
            "A-_A",     // url-safe alphabet
            "====",
        ] {
            assert!(decode_file(bad).is_err(), "accepted {bad:?}");
        }
        assert!(decode_file(&"A".repeat(MAX_VALUE_BYTES + 4)).is_err());
    }
}
