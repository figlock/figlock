//! Per-item keys and per-field encryption (`crypto.md` 7.1).

use zeroize::Zeroizing;

use crate::aead::{open, seal};
use crate::wire::{aad, u32be};
use crate::{Key, Result};

fn ik_aad(vault_id: &[u8; 16], item_id: &[u8; 16], epoch: u32) -> Vec<u8> {
    aad("item-key", &[vault_id, item_id, &u32be(epoch)])
}

fn field_aad(vault_id: &[u8; 16], item_id: &[u8; 16], field_id: &str, hlc: &[u8; 8]) -> Vec<u8> {
    aad("field", &[vault_id, item_id, field_id.as_bytes(), hlc])
}

pub fn wrap_item_key(
    wk: &Key,
    ik: &Key,
    vault_id: &[u8; 16],
    item_id: &[u8; 16],
    epoch: u32,
) -> Result<Vec<u8>> {
    seal(wk, ik.expose(), &ik_aad(vault_id, item_id, epoch))
}

#[cfg(feature = "vector-hooks")]
pub fn wrap_item_key_with_nonce(
    wk: &Key,
    ik: &Key,
    vault_id: &[u8; 16],
    item_id: &[u8; 16],
    epoch: u32,
    nonce: &[u8; 24],
) -> Result<Vec<u8>> {
    crate::aead::seal_with_nonce(wk, ik.expose(), &ik_aad(vault_id, item_id, epoch), nonce)
}

pub fn unwrap_item_key(
    wk: &Key,
    wrapped: &[u8],
    vault_id: &[u8; 16],
    item_id: &[u8; 16],
    epoch: u32,
) -> Result<Key> {
    Key::from_slice(&open(wk, wrapped, &ik_aad(vault_id, item_id, epoch))?)
}

/// Seal one field value (CBOR bytes) under the item key, bound to item, field and write stamp.
pub fn seal_field(
    ik: &Key,
    value_cbor: &[u8],
    vault_id: &[u8; 16],
    item_id: &[u8; 16],
    field_id: &str,
    hlc: &[u8; 8],
) -> Result<Vec<u8>> {
    seal(ik, value_cbor, &field_aad(vault_id, item_id, field_id, hlc))
}

#[cfg(feature = "vector-hooks")]
pub fn seal_field_with_nonce(
    ik: &Key,
    value_cbor: &[u8],
    vault_id: &[u8; 16],
    item_id: &[u8; 16],
    field_id: &str,
    hlc: &[u8; 8],
    nonce: &[u8; 24],
) -> Result<Vec<u8>> {
    crate::aead::seal_with_nonce(
        ik,
        value_cbor,
        &field_aad(vault_id, item_id, field_id, hlc),
        nonce,
    )
}

pub fn open_field(
    ik: &Key,
    blob: &[u8],
    vault_id: &[u8; 16],
    item_id: &[u8; 16],
    field_id: &str,
    hlc: &[u8; 8],
) -> Result<Zeroizing<Vec<u8>>> {
    open(ik, blob, &field_aad(vault_id, item_id, field_id, hlc))
}
