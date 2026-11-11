//! FigLock core: crypto, items, op log, merge and search. Pure logic, no I/O.
//!
//! This crate currently holds the crypto module (`docs/spec/crypto.md`, the container and invite parts of
//! `sync.md` and `pairing-recovery.md`) and the sync logic of `sync.md` (hybrid logical clock, field-level
//! merge, op batch and snapshot payloads, reader decisions), plus local search over non-secret fields
//! (`search`) the Emergency Kit PDF with its verification step (`kit`) and import of other managers' exports (`import`). Every rule is checked against the test vectors in
//! `docs/spec/vectors/` by `tests/vectors.rs` and `tests/sync.rs`.
//!
//! Conventions, from the spec:
//! - one generic [`Error`] for every verification or decryption failure;
//! - secret values live in types that zeroize on drop and do not implement `Debug` or `Display`;
//! - randomness comes only from the OS; failure to get it is an error, never a fallback.

mod aead;
mod audit;
mod document;
mod error;
mod file;
pub mod generate;
mod hlc;
mod import;
mod invite;
mod items;
mod kdf;
mod keys;
mod kit;
mod merge;
mod pairing;
mod payload;
mod quick_se;
mod reader;
mod schema;
mod search;
mod secret;
mod secret_key;
mod signing;
mod vault;
mod wire;

pub use aead::{VERSION, open, seal};
pub use audit::{
    AUDIT_EVENTS, AuditChain, AuditRecord, ZERO_PREV, audit_frame, audit_payload, audit_split,
    clean_text as audit_clean_text, read_audit_chain,
};
pub use document::{MAX_FILE_BYTES, decode_file, encode_file};
pub use error::{Error, Result};
pub use file::{FILE_HEADER_LEN, FileId, Kind, build_file, read_file};
pub use hlc::{Hlc, MAX_DRIFT_MS, Stamp, is_held};
pub use import::{
    Detected, Draft, ImportError, MAX_INPUT_BYTES, MAX_RECORDS, Report, SkipReason, Skipped,
    Source, detect_tokens, parse as parse_import,
};
pub use invite::{
    INBOX_MAX_BYTES, INBOX_STALE_MS, INVITE_TTL_MS, InboxKind, Invite, InviteOffer, JoinAttempt,
    JoinRequest, JoinRequestFile, Joined, device_fingerprint, inbox_is_stale, invite_path,
    parse_inbox_path,
};
pub use items::{open_field, seal_field, unwrap_item_key, wrap_item_key};
pub use kdf::{KdfParams, derive_pw_key};
pub use keys::{
    VaultKeys, derive_auk, derive_vault_keys, epoch_key, open_epoch_key, open_quick_unlock,
    seal_quick_unlock, unwrap_vk, wrap_vk,
};
pub use kit::{KIT_VERSION, KitInfo, render_kit, verify_kit_groups};
pub use merge::{DeviceId, Register, State, Write};
pub use pairing::{
    InviteKeys, invite_code_format, invite_code_parse, invite_keys, open_seal_to, seal_to,
    x25519_public,
};
pub use payload::{ItemId, Op, OpBatch, Snapshot, SnapshotField, SnapshotItem, padded_size};
pub use quick_se::{
    QUICK_SE_POINT_LEN, open_quick_unlock_se, quick_unlock_se_ephemeral_public,
    seal_quick_unlock_se,
};
pub use reader::{
    LogPlan, Warning, accept_from_revoked, accept_vault_json, check_listing, plan_log,
};
pub use schema::{
    ItemType, MAX_FIELDS_PER_ITEM, MAX_FILES_PER_ITEM, MAX_VALUE_BYTES, is_secret_field,
    max_value_bytes, validate_field,
};
pub use search::{Hit, Index};
pub use secret::Key;
pub use secret_key::SecretKey;
pub use signing::{sign_json, verify_json};
pub use vault::{
    Approval, Committed, Created, CredentialChange, DeviceInfo, DeviceKeys, FieldValue, ItemView,
    Revocation, Rotated, Vault,
};
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
    pub use crate::quick_se::seal_quick_unlock_se_with;
    pub use crate::signing::canonical_json;
}
