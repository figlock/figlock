//! FigLock core: crypto, items, op log, merge and search. Pure logic, no I/O.
//!
//! This crate currently holds the crypto module (`docs/spec/crypto.md`, the container and invite parts of
//! `sync.md` and `pairing-recovery.md`) and the sync logic of `sync.md` (hybrid logical clock, field-level
//! merge, op batch and snapshot payloads, reader decisions). Every rule is checked against the test vectors in
//! `docs/spec/vectors/` by `tests/vectors.rs` and `tests/sync.rs`.
//!
//! Conventions, from the spec:
//! - one generic [`Error`] for every verification or decryption failure;
//! - secret values live in types that zeroize on drop and do not implement `Debug` or `Display`;
//! - randomness comes only from the OS; failure to get it is an error, never a fallback.

mod aead;
mod error;
mod file;
mod hlc;
mod items;
mod kdf;
mod keys;
mod merge;
mod pairing;
mod payload;
mod reader;
mod secret;
mod secret_key;
mod signing;
mod wire;

pub use aead::{VERSION, open, seal};
pub use error::{Error, Result};
pub use file::{FILE_HEADER_LEN, FileId, Kind, build_file, read_file};
pub use hlc::{Hlc, MAX_DRIFT_MS, Stamp, is_held};
pub use items::{open_field, seal_field, unwrap_item_key, wrap_item_key};
pub use kdf::{KdfParams, derive_pw_key};
pub use keys::{
    VaultKeys, derive_auk, derive_vault_keys, epoch_key, open_epoch_key, open_quick_unlock,
    seal_quick_unlock, unwrap_vk, wrap_vk,
};
pub use merge::{DeviceId, Register, State, Write};
pub use pairing::{
    InviteKeys, invite_code_format, invite_code_parse, invite_keys, open_seal_to, seal_to,
    x25519_public,
};
pub use payload::{ItemId, Op, OpBatch, Snapshot, SnapshotField, SnapshotItem, padded_size};
pub use reader::{
    LogPlan, Warning, accept_from_revoked, accept_vault_json, check_listing, plan_log,
};
pub use secret::Key;
pub use secret_key::SecretKey;
pub use signing::{sign_json, verify_json};
pub use wire::{aad, hlc_bytes, u16be, u32be, u64be};

/// Deterministic entry points that take explicit nonces and ephemeral keys, for the test vectors only.
/// Reusing a nonce with a key breaks the cipher, so this module exists only with the `vector-hooks`
/// feature, which must never be enabled in a shipped binary.
#[cfg(feature = "vector-hooks")]
pub mod vector_hooks {
    pub use crate::aead::seal_with_nonce;
    pub use crate::file::build_file_with_nonce;
    pub use crate::items::{seal_field_with_nonce, wrap_item_key_with_nonce};
    pub use crate::kdf::hkdf_raw;
    pub use crate::keys::{epoch_key_with_nonce, seal_quick_unlock_with_nonce, wrap_vk_with_nonce};
    pub use crate::pairing::seal_to_with;
    pub use crate::signing::canonical_json;
}
