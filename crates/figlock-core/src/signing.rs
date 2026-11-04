//! Signed JSON: device records and `vault.json` (`crypto.md` 7.4).
//!
//! The signed bytes are `"figlock/v1/<context>" || 0x00 || JCS(object)`. JCS (RFC 8785) is produced by
//! an explicit serializer that sorts object keys itself. That is exact for the value types FigLock signs:
//! strings, integers within +-2^53, booleans, arrays and objects with ASCII keys. Floats, `null` and larger
//! integers are rejected so no ambiguity can arise.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde_json::Value;

use crate::{Error, Key, Result};

const MAX_SAFE_INT: u64 = 1 << 53;

fn write_string(s: &str, out: &mut Vec<u8>) -> Result<()> {
    serde_json::to_writer(out, s).map_err(|_| Error)
}

fn write_jcs(v: &Value, out: &mut Vec<u8>) -> Result<()> {
    match v {
        Value::Null => Err(Error),
        Value::Bool(b) => {
            out.extend_from_slice(if *b { b"true" } else { b"false" });
            Ok(())
        }
        Value::Number(n) => {
            let in_range = n
                .as_u64()
                .map(|u| u <= MAX_SAFE_INT)
                .or_else(|| n.as_i64().map(|i| i.unsigned_abs() <= MAX_SAFE_INT))
                .unwrap_or(false);
            if !in_range {
                return Err(Error);
            }
            out.extend_from_slice(n.to_string().as_bytes());
            Ok(())
        }
        Value::String(s) => write_string(s, out),
        Value::Array(a) => {
            out.push(b'[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_jcs(item, out)?;
            }
            out.push(b']');
            Ok(())
        }
        Value::Object(m) => {
            if !m.keys().all(|k| k.is_ascii()) {
                return Err(Error);
            }
            let mut entries: Vec<_> = m.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push(b'{');
            for (i, (k, val)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(k, out)?;
                out.push(b':');
                write_jcs(val, out)?;
            }
            out.push(b'}');
            Ok(())
        }
    }
}

fn jcs(value: &Value) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    write_jcs(value, &mut out)?;
    Ok(out)
}

fn message(context: &str, value: &Value) -> Result<Vec<u8>> {
    let mut m = format!("figlock/v1/{context}").into_bytes();
    m.push(0);
    m.extend_from_slice(&jcs(value)?);
    Ok(m)
}

/// The object MUST NOT contain its own signature member; sign the rest and store the signature beside it.
pub fn sign_json(authority_seed: &Key, context: &str, value: &Value) -> Result<[u8; 64]> {
    let sk = SigningKey::from_bytes(authority_seed.expose());
    Ok(sk.sign(&message(context, value)?).to_bytes())
}

pub fn verify_json(
    authority_pub: &[u8; 32],
    context: &str,
    value: &Value,
    signature: &[u8; 64],
) -> Result<()> {
    let vk = VerifyingKey::from_bytes(authority_pub).map_err(|_| Error)?;
    vk.verify_strict(&message(context, value)?, &Signature::from_bytes(signature))
        .map_err(|_| Error)
}

/// The canonical form, exposed so vectors can pin it.
#[cfg(feature = "vector-hooks")]
pub fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    jcs(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    #[test]
    fn keys_are_sorted_regardless_of_insertion_order() {
        let mut m = Map::new();
        m.insert("b".into(), json!(1));
        m.insert("a".into(), json!({"z": true, "y": [2, "x"]}));
        m.insert("B".into(), json!(false));
        let out = jcs(&Value::Object(m)).unwrap();
        assert_eq!(out, br#"{"B":false,"a":{"y":[2,"x"],"z":true},"b":1}"#);
    }

    #[test]
    fn integers_beyond_2_pow_53_are_rejected() {
        assert!(jcs(&json!(1u64 << 53)).is_ok());
        assert!(jcs(&json!(-(1i64 << 53))).is_ok());
        assert!(jcs(&json!((1u64 << 53) + 1)).is_err());
        assert!(jcs(&json!(-(1i64 << 53) - 1)).is_err());
        assert!(jcs(&json!(u64::MAX)).is_err());
        assert!(jcs(&json!(i64::MIN)).is_err());
    }

    #[test]
    fn floats_null_and_non_ascii_keys_are_rejected() {
        assert!(jcs(&json!(1.5)).is_err());
        assert!(jcs(&json!({"a": null})).is_err());
        assert!(jcs(&json!({"é": 1})).is_err());
    }
}
