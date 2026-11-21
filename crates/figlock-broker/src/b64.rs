//! Base64 (RFC 4648), for response bodies on the broker socket and for recognising encoded secrets in output.

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encode with the standard alphabet and padding.
pub fn encode(bytes: &[u8]) -> String {
    String::from_utf8(encode_with(bytes, false, true)).unwrap_or_default()
}

/// Encode with the chosen alphabet, with or without `=` padding.
pub fn encode_with(bytes: &[u8], url_safe: bool, pad: bool) -> Vec<u8> {
    let table = if url_safe { URL } else { STD };
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(table[(n >> 18) as usize & 63]);
        out.push(table[(n >> 12) as usize & 63]);
        if c.len() > 1 {
            out.push(table[(n >> 6) as usize & 63]);
        } else if pad {
            out.push(b'=');
        }
        if c.len() > 2 {
            out.push(table[n as usize & 63]);
        } else if pad {
            out.push(b'=');
        }
    }
    out
}

/// Decode the standard alphabet with padding. Anything else is `None`.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let s = s.as_bytes();
    if !s.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for (i, quad) in s.chunks(4).enumerate() {
        let last = i == s.len() / 4 - 1;
        let pad = quad.iter().rev().take_while(|b| **b == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for (j, b) in quad.iter().enumerate() {
            let v = if j >= 4 - pad {
                0
            } else {
                u32::try_from(STD.iter().position(|t| t == b)?).ok()?
            };
            n = (n << 6) | v;
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_4648_vectors() {
        for (plain, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), enc);
            assert_eq!(decode(enc).unwrap(), plain.as_bytes());
        }
    }

    #[test]
    fn alphabets_and_padding_options() {
        assert_eq!(encode_with(&[0xfb, 0xff], false, false), b"+/8");
        assert_eq!(encode_with(&[0xfb, 0xff], true, false), b"-_8");
        assert_eq!(encode_with(b"M", false, false), b"TQ");
    }

    #[test]
    fn decode_refuses_what_is_not_base64() {
        for bad in ["Zg=", "Zg", "Z===", "=Zg=", "Zm9v!A==", "Zg==Zm9v", "Zm9 v"] {
            assert!(decode(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn every_length_round_trips() {
        let data: Vec<u8> = (0..=255u8).collect();
        for n in 0..data.len() {
            assert_eq!(decode(&encode(&data[..n])).unwrap(), &data[..n]);
        }
    }
}
