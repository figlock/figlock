//! Quick unlock on the foreign side (`crypto.md` 8, ADR-0025, ADR-0041 decision 6).
//!
//! The phone owns `QK`, a Secure Enclave key, and does the one ECDH step the chip allows. Everything else is the
//! wrap of `quick_unlock_se` in `figlock-core`, so the Swift side adds no crypto of its own: it hands over the
//! public point to seal to, and the ECDH result to open with. The Account Unlock Key (`AUK`) never crosses the
//! boundary as bytes; it lives behind [`UnlockKey`].

use std::sync::Arc;

use figlock_core::{
    Key, SecretKey, Vault, open_quick_unlock_se, quick_unlock_se_ephemeral_public,
    seal_quick_unlock_se,
};
use zeroize::Zeroizing;

use crate::device::DeviceKeysHandle;
use crate::error::{FigLockError, Result, parse_id};
use crate::session::{UnlockOutcome, VaultSession};

/// The Account Unlock Key of one vault, held in Rust. It comes from the master password and Secret Key
/// ([`derive_unlock_key`]) or from a quick unlock blob ([`open_quick_unlock`]), and it can open the vault
/// ([`unlock_vault_with_key`]) or be sealed for the next quick unlock ([`UnlockKey::seal_for_quick_unlock`]).
#[derive(uniffi::Object)]
pub struct UnlockKey {
    key: Key,
}

#[uniffi::export]
impl UnlockKey {
    /// Wrap this key to the Secure Enclave key whose public point is `qk_public` (65 bytes, uncompressed), for
    /// the vault and device named by their 32 hex character ids. Needs no prompt. The result is the `qu_blob`
    /// to store in the container.
    pub fn seal_for_quick_unlock(
        &self,
        qk_public: Vec<u8>,
        vault_id: String,
        device_id: String,
    ) -> Result<Vec<u8>> {
        let vault_id = parse_id(&vault_id)?;
        let device_id = parse_id(&device_id)?;
        Ok(seal_quick_unlock_se(
            &qk_public, &self.key, &vault_id, &device_id,
        )?)
    }
}

/// Derive the unlock key from the master password and Secret Key text (`FL1-...`). A wrong password, a wrong
/// Secret Key and a tampered file are not told apart here: the wrong ones fail later, at
/// [`unlock_vault_with_key`], with the same `Failed`.
#[uniffi::export]
pub fn derive_unlock_key(
    vault_json: Vec<u8>,
    password: String,
    secret_key: String,
) -> Result<Arc<UnlockKey>> {
    let password = Zeroizing::new(password);
    let secret_key = Zeroizing::new(secret_key);
    let sk = SecretKey::parse(&secret_key).map_err(|_| FigLockError::InvalidInput)?;
    let key = Vault::unlock_key(&vault_json, &password, &sk)?;
    Ok(Arc::new(UnlockKey { key }))
}

/// Open the vault with an unlock key from [`derive_unlock_key`] or [`open_quick_unlock`]. A wrong key gives
/// `Failed`, like a wrong password.
#[uniffi::export]
pub fn unlock_vault_with_key(
    vault_json: Vec<u8>,
    key: Arc<UnlockKey>,
    device: Arc<DeviceKeysHandle>,
    device_name: String,
    device_records: Vec<Vec<u8>>,
    now_ms: u64,
) -> Result<UnlockOutcome> {
    let (vault, record) = Vault::unlock_with_key(
        &vault_json,
        &key.key,
        device.to_core(),
        &device_name,
        &device_records,
        now_ms,
    )?;
    Ok(UnlockOutcome {
        session: Arc::new(VaultSession::new(vault)),
        new_device_record: record,
    })
}

/// The ephemeral public point inside a `qu_blob` (65 bytes), which the Secure Enclave needs to compute the
/// shared secret. A damaged or foreign blob fails here, before the chip asks for Face ID.
#[uniffi::export]
pub fn quick_unlock_ephemeral_public(blob: Vec<u8>) -> Result<Vec<u8>> {
    Ok(quick_unlock_se_ephemeral_public(&blob)?.to_vec())
}

/// Unwrap a `qu_blob`. `shared` is the 32 byte ECDH result of `QK` and the blob's ephemeral point,
/// `qk_public` is `QK`'s public point. Wrong ids, a flipped bit and a key from another chip all give `Failed`.
#[uniffi::export]
pub fn open_quick_unlock(
    blob: Vec<u8>,
    qk_public: Vec<u8>,
    shared: Vec<u8>,
    vault_id: String,
    device_id: String,
) -> Result<Arc<UnlockKey>> {
    let shared = Zeroizing::new(shared);
    let vault_id = parse_id(&vault_id)?;
    let device_id = parse_id(&device_id)?;
    let key = open_quick_unlock_se(&blob, &qk_public, &shared, &vault_id, &device_id)?;
    Ok(Arc::new(UnlockKey { key }))
}
