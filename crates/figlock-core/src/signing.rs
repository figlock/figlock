//! Signed JSON: device records and `vault.json` (`crypto.md` 7.4).
//!
//! The signed bytes are `"figlock/v1/<context>" || 0x00 || JCS(object)`. JCS (RFC 8785) is produced by
//! `serde_json`, whose maps are sorted. That is exact for the value types FigLock signs: strings, integers,
//! booleans, arrays and objects with ASCII keys. Floats and `null` are rejected so no ambiguity can arise.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde_json::Value;

use crate::{Error, Key, Result};

fn jcs(value: &Value) -> Result<Vec<u8>> {
    fn check(v: &Value) -> Result<()> {
        match v {
            Value::Null => Err(Error),
            Value::Number(n) if !(n.is_u64() || n.is_i64()) => Err(Error),
            Value::Array(a) => a.iter().try_for_each(check),
            Value::Object(m) => m
                .iter()
                .try_for_each(|(k, v)| if k.is_ascii() { check(v) } else { Err(Error) }),
            _ => Ok(()),
        }
    }
    check(value)?;
    serde_json::to_vec(value).map_err(|_| Error)
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
