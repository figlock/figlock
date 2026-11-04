//! Key hierarchy: `AUK`, `VK`, derived keys, rotation and quick unlock (`crypto.md` 5, 6, 8, 9).

use ed25519_dalek::SigningKey;
use zeroize::Zeroizing;

use crate::aead::{open, seal};
use crate::kdf::{KdfParams, derive_pw_key, hkdf_key};
use crate::wire::{aad, u32be};
use crate::{Key, Result, SecretKey};

/// `AUK = HKDF(pw_key || sk_key, salt = vault_id, "figlock/v1/account-unlock-key")`.
pub fn derive_auk(
    password: &str,
    secret_key: &SecretKey,
    vault_id: &[u8; 16],
    salt: &[u8; 16],
    params: &KdfParams,
) -> Result<Key> {
    let pw_key = derive_pw_key(password, salt, params)?;
    let sk_key = hkdf_key(secret_key.expose(), Some(vault_id), "figlock/v1/secret-key")?;
    let mut ikm = Zeroizing::new([0u8; 64]);
    ikm[..32].copy_from_slice(pw_key.expose());
    ikm[32..].copy_from_slice(sk_key.expose());
    hkdf_key(
        ikm.as_ref(),
        Some(vault_id),
        "figlock/v1/account-unlock-key",
    )
}

/// Keys derived from the Vault Key (`crypto.md` 6).
pub struct VaultKeys {
    /// Op and snapshot encryption key.
    pub lk: Key,
    /// Item key wrapping key.
    pub wk: Key,
    /// Ed25519 seed of the authority key, the vault's trust anchor.
    pub authority_seed: Key,
}

impl VaultKeys {
    pub fn authority_public(&self) -> [u8; 32] {
        SigningKey::from_bytes(self.authority_seed.expose())
            .verifying_key()
            .to_bytes()
    }
}

pub fn derive_vault_keys(vk: &Key) -> Result<VaultKeys> {
    Ok(VaultKeys {
        lk: hkdf_key(vk.expose(), None, "figlock/v1/log-key")?,
        wk: hkdf_key(vk.expose(), None, "figlock/v1/item-key-wrap")?,
        authority_seed: hkdf_key(vk.expose(), None, "figlock/v1/authority-seed")?,
    })
}

fn vk_aad(vault_id: &[u8; 16], epoch: u32, params_bytes: &[u8]) -> Vec<u8> {
    aad("vk-wrap", &[vault_id, &u32be(epoch), params_bytes])
}

pub fn wrap_vk(
    auk: &Key,
    vk: &Key,
    vault_id: &[u8; 16],
    epoch: u32,
    params_bytes: &[u8],
) -> Result<Vec<u8>> {
    seal(auk, vk.expose(), &vk_aad(vault_id, epoch, params_bytes))
}

#[cfg(feature = "vector-hooks")]
pub fn wrap_vk_with_nonce(
    auk: &Key,
    vk: &Key,
    vault_id: &[u8; 16],
    epoch: u32,
    params_bytes: &[u8],
    nonce: &[u8; 24],
) -> Result<Vec<u8>> {
    crate::aead::seal_with_nonce(
        auk,
        vk.expose(),
        &vk_aad(vault_id, epoch, params_bytes),
        nonce,
    )
}

pub fn unwrap_vk(
    auk: &Key,
    wrapped: &[u8],
    vault_id: &[u8; 16],
    epoch: u32,
    params_bytes: &[u8],
) -> Result<Key> {
    Key::from_slice(&open(auk, wrapped, &vk_aad(vault_id, epoch, params_bytes))?)
}

fn epoch_aad(vault_id: &[u8; 16], old_epoch: u32, new_epoch: u32) -> Vec<u8> {
    aad(
        "epoch-key",
        &[vault_id, &u32be(old_epoch), &u32be(new_epoch)],
    )
}

/// `seal(VK', VK_old, aad("epoch-key", vault_id, old_epoch, new_epoch))` (`crypto.md` 9.1).
pub fn epoch_key(
    new_vk: &Key,
    old_vk: &Key,
    vault_id: &[u8; 16],
    old_epoch: u32,
    new_epoch: u32,
) -> Result<Vec<u8>> {
    seal(
        new_vk,
        old_vk.expose(),
        &epoch_aad(vault_id, old_epoch, new_epoch),
    )
}

#[cfg(feature = "vector-hooks")]
pub fn epoch_key_with_nonce(
    new_vk: &Key,
    old_vk: &Key,
    vault_id: &[u8; 16],
    old_epoch: u32,
    new_epoch: u32,
    nonce: &[u8; 24],
) -> Result<Vec<u8>> {
    crate::aead::seal_with_nonce(
        new_vk,
        old_vk.expose(),
        &epoch_aad(vault_id, old_epoch, new_epoch),
        nonce,
    )
}

pub fn open_epoch_key(
    new_vk: &Key,
    sealed: &[u8],
    vault_id: &[u8; 16],
    old_epoch: u32,
    new_epoch: u32,
) -> Result<Key> {
    Key::from_slice(&open(
        new_vk,
        sealed,
        &epoch_aad(vault_id, old_epoch, new_epoch),
    )?)
}

fn qu_aad(vault_id: &[u8; 16], device_id: &[u8; 16]) -> Vec<u8> {
    aad("quick-unlock", &[vault_id, device_id])
}

/// Software-key variant of the quick-unlock blob (`crypto.md` 8). Hardware wraps are tested on devices.
pub fn seal_quick_unlock(
    qk: &Key,
    auk: &Key,
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
) -> Result<Vec<u8>> {
    seal(qk, auk.expose(), &qu_aad(vault_id, device_id))
}

#[cfg(feature = "vector-hooks")]
pub fn seal_quick_unlock_with_nonce(
    qk: &Key,
    auk: &Key,
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
    nonce: &[u8; 24],
) -> Result<Vec<u8>> {
    crate::aead::seal_with_nonce(qk, auk.expose(), &qu_aad(vault_id, device_id), nonce)
}

pub fn open_quick_unlock(
    qk: &Key,
    blob: &[u8],
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
) -> Result<Key> {
    Key::from_slice(&open(qk, blob, &qu_aad(vault_id, device_id))?)
}
